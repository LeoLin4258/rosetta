use std::{
    collections::BTreeMap,
    fmt,
    fs::{self, File, OpenOptions},
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    object_delta::PdfObjectDelta,
    source_object::{PdfObjectView, PdfSourceObjectStore},
};

const SOURCE_COPY_BUFFER_BYTES: usize = 64 * 1024;
const MAX_CLASSIC_XREF_OFFSET: u64 = u32::MAX as u64;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub(crate) struct IncrementalExportBase {
    source_fingerprint: String,
    source_bytes: u64,
    previous_xref_offset: u64,
    maximum_object_number: u32,
    trailer: Dictionary,
}

impl IncrementalExportBase {
    pub(crate) fn from_document(
        source_fingerprint: impl Into<String>,
        source_bytes: u64,
        document: &Document,
    ) -> Result<Self, IncrementalExportError> {
        let source_fingerprint = source_fingerprint.into();
        if !is_sha256_fingerprint(&source_fingerprint) {
            return Err(IncrementalExportError::InvalidSourceFingerprint);
        }
        if source_bytes == 0 {
            return Err(IncrementalExportError::InvalidSourceLength);
        }
        if document.xref_start == 0
            || u64::try_from(document.xref_start).map_or(true, |offset| offset >= source_bytes)
        {
            return Err(IncrementalExportError::InvalidPreviousXrefOffset);
        }
        if document
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .is_err()
        {
            return Err(IncrementalExportError::MissingCatalog);
        }
        if document.trailer.has(b"Encrypt") || document.is_encrypted() {
            return Err(IncrementalExportError::EncryptedDocument);
        }
        Ok(Self {
            source_fingerprint,
            source_bytes,
            previous_xref_offset: u64::try_from(document.xref_start)
                .map_err(|_| IncrementalExportError::InvalidPreviousXrefOffset)?,
            maximum_object_number: document.max_id,
            trailer: document.trailer.clone(),
        })
    }

    pub(crate) fn from_source_object_store(
        source_fingerprint: impl Into<String>,
        source: &PdfSourceObjectStore,
    ) -> Result<Self, IncrementalExportError> {
        let source_fingerprint = source_fingerprint.into();
        if !is_sha256_fingerprint(&source_fingerprint) {
            return Err(IncrementalExportError::InvalidSourceFingerprint);
        }
        if source.source_bytes() == 0 {
            return Err(IncrementalExportError::InvalidSourceLength);
        }
        if source.previous_xref_offset() == 0
            || source.previous_xref_offset() >= source.source_bytes()
        {
            return Err(IncrementalExportError::InvalidPreviousXrefOffset);
        }
        if source
            .trailer()
            .get(b"Root")
            .and_then(Object::as_reference)
            .is_err()
        {
            return Err(IncrementalExportError::MissingCatalog);
        }
        if source.trailer().has(b"Encrypt") {
            return Err(IncrementalExportError::EncryptedDocument);
        }
        Ok(Self {
            source_fingerprint,
            source_bytes: source.source_bytes(),
            previous_xref_offset: source.previous_xref_offset(),
            maximum_object_number: source.maximum_object_number(),
            trailer: source.trailer().clone(),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct IncrementalExportCancellation {
    cancelled: Arc<AtomicBool>,
}

impl IncrementalExportCancellation {
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceCopyExportResult {
    pub schema: &'static str,
    pub source_bytes: u64,
    pub output_bytes: u64,
    pub source_copy_buffer_bytes: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IncrementalExportResult {
    pub schema: &'static str,
    pub source_bytes: u64,
    pub appended_bytes: u64,
    pub output_bytes: u64,
    pub delta_object_count: usize,
    pub previous_xref_offset: u64,
    pub output_xref_offset: u64,
    pub source_copy_buffer_bytes: usize,
}

#[derive(Debug)]
pub(crate) enum IncrementalExportError {
    InvalidSourceFingerprint,
    InvalidSourceLength,
    InvalidPreviousXrefOffset,
    MissingCatalog,
    EncryptedDocument,
    EmptyDelta,
    InvalidObjectId(ObjectId),
    DuplicateObjectNumber(u32),
    SourceDestinationConflict,
    SourceLengthMismatch {
        expected: u64,
        actual: u64,
    },
    SourceFingerprintMismatch,
    OffsetOverflow,
    Cancelled,
    Io {
        operation: &'static str,
        path: PathBuf,
        message: String,
    },
}

impl fmt::Display for IncrementalExportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSourceFingerprint => {
                formatter.write_str("incremental export source fingerprint is invalid")
            }
            Self::InvalidSourceLength => {
                formatter.write_str("incremental export source length is invalid")
            }
            Self::InvalidPreviousXrefOffset => {
                formatter.write_str("incremental export previous xref offset is invalid")
            }
            Self::MissingCatalog => {
                formatter.write_str("incremental export source trailer has no catalog")
            }
            Self::EncryptedDocument => {
                formatter.write_str("encrypted PDFs are not supported by incremental export")
            }
            Self::EmptyDelta => formatter.write_str("incremental export delta is empty"),
            Self::InvalidObjectId((number, generation)) => write!(
                formatter,
                "incremental export object ID {number} {generation} is invalid"
            ),
            Self::DuplicateObjectNumber(number) => write!(
                formatter,
                "incremental export contains multiple generations for object {number}"
            ),
            Self::SourceDestinationConflict => {
                formatter.write_str("incremental export cannot replace its source PDF")
            }
            Self::SourceLengthMismatch { expected, actual } => write!(
                formatter,
                "incremental export source length changed: expected {expected}, found {actual}"
            ),
            Self::SourceFingerprintMismatch => {
                formatter.write_str("incremental export source fingerprint changed")
            }
            Self::OffsetOverflow => {
                formatter.write_str("incremental export exceeds classic xref offset limits")
            }
            Self::Cancelled => formatter.write_str("incremental PDF export was cancelled"),
            Self::Io {
                operation,
                path,
                message,
            } => write!(
                formatter,
                "failed to {operation} incremental PDF export {}: {message}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for IncrementalExportError {}

pub(crate) fn export_incremental_pdf_atomic(
    source_path: impl AsRef<Path>,
    destination_path: impl AsRef<Path>,
    base: &IncrementalExportBase,
    delta: &PdfObjectDelta,
    cancellation: &IncrementalExportCancellation,
) -> Result<IncrementalExportResult, IncrementalExportError> {
    let source_path = source_path.as_ref();
    let destination_path = destination_path.as_ref();
    validate_paths(source_path, destination_path)?;
    let delta_objects = delta.objects();
    validate_delta(delta_objects)?;
    check_cancelled(cancellation)?;

    let destination_parent = path_parent(destination_path);
    let temp_path = unique_sidecar_path(destination_path, "tmp");
    let mut temp_guard = TempFileGuard::new(temp_path.clone());
    let mut source =
        File::open(source_path).map_err(|error| io_error("open source for", source_path, error))?;
    let temp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| io_error("create temporary", &temp_path, error))?;
    let mut writer = BufWriter::with_capacity(SOURCE_COPY_BUFFER_BYTES, temp);
    let (copied_bytes, source_fingerprint, last_source_byte) = copy_source(
        &mut source,
        &mut writer,
        source_path,
        &temp_path,
        cancellation,
    )?;
    if copied_bytes != base.source_bytes {
        return Err(IncrementalExportError::SourceLengthMismatch {
            expected: base.source_bytes,
            actual: copied_bytes,
        });
    }
    if source_fingerprint != base.source_fingerprint {
        return Err(IncrementalExportError::SourceFingerprintMismatch);
    }

    let mut output = CountingWriter::new(&mut writer, copied_bytes);
    if last_source_byte != Some(b'\n') && last_source_byte != Some(b'\r') {
        output
            .write_all(b"\n")
            .map_err(|error| io_error("write temporary", &temp_path, error))?;
    }
    output
        .write_all(b"% Rosetta PDF v3 incremental export\n")
        .map_err(|error| io_error("write temporary", &temp_path, error))?;

    let mut xref_entries = Vec::with_capacity(delta_objects.len());
    for (&(object_number, generation), object) in delta_objects {
        check_cancelled(cancellation)?;
        let offset = output.bytes_written();
        validate_xref_offset(offset)?;
        write!(output, "{object_number} {generation} obj\n")
            .map_err(|error| io_error("write temporary", &temp_path, error))?;
        write_pdf_object(&mut output, object)
            .map_err(|error| io_error("write temporary", &temp_path, error))?;
        output
            .write_all(b"\nendobj\n")
            .map_err(|error| io_error("write temporary", &temp_path, error))?;
        xref_entries.push((object_number, generation, offset));
    }

    check_cancelled(cancellation)?;
    let output_xref_offset = output.bytes_written();
    validate_xref_offset(output_xref_offset)?;
    output
        .write_all(b"xref\n")
        .map_err(|error| io_error("write temporary", &temp_path, error))?;
    write_xref_sections(&mut output, &xref_entries)
        .map_err(|error| io_error("write temporary", &temp_path, error))?;

    let maximum_object_number = base
        .maximum_object_number
        .max(delta.maximum_object_number());
    let trailer = incremental_trailer(base, maximum_object_number)?;
    output
        .write_all(b"trailer\n")
        .map_err(|error| io_error("write temporary", &temp_path, error))?;
    write_pdf_object(&mut output, &Object::Dictionary(trailer))
        .map_err(|error| io_error("write temporary", &temp_path, error))?;
    write!(output, "\nstartxref\n{output_xref_offset}\n%%EOF\n")
        .map_err(|error| io_error("write temporary", &temp_path, error))?;
    let output_bytes = output.bytes_written();
    drop(output);
    writer
        .flush()
        .map_err(|error| io_error("flush temporary", &temp_path, error))?;
    let file = writer
        .into_inner()
        .map_err(|error| io_error("finish temporary", &temp_path, error.into_error()))?;
    file.sync_all()
        .map_err(|error| io_error("sync temporary", &temp_path, error))?;

    check_cancelled(cancellation)?;
    replace_file_atomic(&temp_path, destination_path)
        .map_err(|error| io_error("commit", destination_path, error))?;
    temp_guard.disarm();
    sync_parent_directory(destination_parent)
        .map_err(|error| io_error("sync destination parent for", destination_path, error))?;

    Ok(IncrementalExportResult {
        schema: "rosetta-pdf-v3-incremental-export/1",
        source_bytes: copied_bytes,
        appended_bytes: output_bytes.saturating_sub(copied_bytes),
        output_bytes,
        delta_object_count: delta.object_count(),
        previous_xref_offset: base.previous_xref_offset,
        output_xref_offset,
        source_copy_buffer_bytes: SOURCE_COPY_BUFFER_BYTES,
    })
}

pub(crate) fn export_source_pdf_atomic(
    source_path: impl AsRef<Path>,
    destination_path: impl AsRef<Path>,
    base: &IncrementalExportBase,
    cancellation: &IncrementalExportCancellation,
) -> Result<SourceCopyExportResult, IncrementalExportError> {
    let source_path = source_path.as_ref();
    let destination_path = destination_path.as_ref();
    validate_paths(source_path, destination_path)?;
    check_cancelled(cancellation)?;

    let destination_parent = path_parent(destination_path);
    let temp_path = unique_sidecar_path(destination_path, "tmp");
    let mut temp_guard = TempFileGuard::new(temp_path.clone());
    let mut source =
        File::open(source_path).map_err(|error| io_error("open source for", source_path, error))?;
    let temp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|error| io_error("create temporary", &temp_path, error))?;
    let mut writer = BufWriter::with_capacity(SOURCE_COPY_BUFFER_BYTES, temp);
    let (copied_bytes, source_fingerprint, _) = copy_source(
        &mut source,
        &mut writer,
        source_path,
        &temp_path,
        cancellation,
    )?;
    if copied_bytes != base.source_bytes {
        return Err(IncrementalExportError::SourceLengthMismatch {
            expected: base.source_bytes,
            actual: copied_bytes,
        });
    }
    if source_fingerprint != base.source_fingerprint {
        return Err(IncrementalExportError::SourceFingerprintMismatch);
    }
    writer
        .flush()
        .map_err(|error| io_error("flush temporary", &temp_path, error))?;
    let file = writer
        .into_inner()
        .map_err(|error| io_error("finish temporary", &temp_path, error.into_error()))?;
    file.sync_all()
        .map_err(|error| io_error("sync temporary", &temp_path, error))?;

    check_cancelled(cancellation)?;
    replace_file_atomic(&temp_path, destination_path)
        .map_err(|error| io_error("commit", destination_path, error))?;
    temp_guard.disarm();
    sync_parent_directory(destination_parent)
        .map_err(|error| io_error("sync destination parent for", destination_path, error))?;

    Ok(SourceCopyExportResult {
        schema: "rosetta-pdf-v3-source-copy-export/1",
        source_bytes: copied_bytes,
        output_bytes: copied_bytes,
        source_copy_buffer_bytes: SOURCE_COPY_BUFFER_BYTES,
    })
}

fn validate_paths(source: &Path, destination: &Path) -> Result<(), IncrementalExportError> {
    let source =
        fs::canonicalize(source).map_err(|error| io_error("resolve source for", source, error))?;
    let destination_parent = path_parent(destination);
    let destination_parent = fs::canonicalize(destination_parent)
        .map_err(|error| io_error("resolve destination parent for", destination, error))?;
    let destination_name = destination.file_name().ok_or_else(|| {
        io_error(
            "resolve destination filename for",
            destination,
            io::Error::new(io::ErrorKind::InvalidInput, "destination has no filename"),
        )
    })?;
    let destination = if destination.exists() {
        fs::canonicalize(destination)
            .map_err(|error| io_error("resolve destination for", destination, error))?
    } else {
        destination_parent.join(destination_name)
    };
    if paths_equal(&source, &destination) {
        return Err(IncrementalExportError::SourceDestinationConflict);
    }
    Ok(())
}

fn path_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[cfg(windows)]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(windows))]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

fn validate_delta(
    delta_objects: &BTreeMap<ObjectId, Object>,
) -> Result<(), IncrementalExportError> {
    if delta_objects.is_empty() {
        return Err(IncrementalExportError::EmptyDelta);
    }
    let mut previous_number = None;
    for &(number, generation) in delta_objects.keys() {
        if number == 0 || generation == u16::MAX {
            return Err(IncrementalExportError::InvalidObjectId((
                number, generation,
            )));
        }
        if previous_number == Some(number) {
            return Err(IncrementalExportError::DuplicateObjectNumber(number));
        }
        previous_number = Some(number);
    }
    Ok(())
}

fn copy_source(
    source: &mut File,
    destination: &mut BufWriter<File>,
    source_path: &Path,
    destination_path: &Path,
    cancellation: &IncrementalExportCancellation,
) -> Result<(u64, String, Option<u8>), IncrementalExportError> {
    let mut buffer = [0_u8; SOURCE_COPY_BUFFER_BYTES];
    let mut copied = 0_u64;
    let mut hasher = Sha256::new();
    let mut last_byte = None;
    loop {
        check_cancelled(cancellation)?;
        let read = source
            .read(&mut buffer)
            .map_err(|error| io_error("read source for", source_path, error))?;
        if read == 0 {
            break;
        }
        destination
            .write_all(&buffer[..read])
            .map_err(|error| io_error("copy source to", destination_path, error))?;
        hasher.update(&buffer[..read]);
        copied = copied
            .checked_add(u64::try_from(read).unwrap_or(u64::MAX))
            .ok_or(IncrementalExportError::OffsetOverflow)?;
        last_byte = buffer.get(read - 1).copied();
    }
    Ok((
        copied,
        format!("sha256:{}", hex_digest(&hasher.finalize())),
        last_byte,
    ))
}

fn write_xref_sections(output: &mut impl Write, entries: &[(u32, u16, u64)]) -> io::Result<()> {
    let mut start = 0;
    while start < entries.len() {
        let mut end = start + 1;
        while end < entries.len() && entries[end].0 == entries[end - 1].0 + 1 {
            end += 1;
        }
        writeln!(output, "{} {}", entries[start].0, end - start)?;
        for &(_, generation, offset) in &entries[start..end] {
            writeln!(output, "{offset:010} {generation:05} n ")?;
        }
        start = end;
    }
    Ok(())
}

fn write_pdf_object(output: &mut impl Write, object: &Object) -> io::Result<()> {
    match object {
        Object::Null => output.write_all(b"null"),
        Object::Boolean(true) => output.write_all(b"true"),
        Object::Boolean(false) => output.write_all(b"false"),
        Object::Integer(value) => write!(output, "{value}"),
        Object::Real(value) => write!(output, "{value}"),
        Object::Name(name) => write_pdf_name(output, name),
        Object::String(value, format) => write_pdf_string(output, value, format),
        Object::Array(values) => {
            output.write_all(b"[")?;
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    output.write_all(b" ")?;
                }
                write_pdf_object(output, value)?;
            }
            output.write_all(b"]")
        }
        Object::Dictionary(dictionary) => write_pdf_dictionary(output, dictionary),
        Object::Stream(stream) => {
            let mut dictionary = stream.dict.clone();
            dictionary.set(
                "Length",
                i64::try_from(stream.content.len())
                    .map_err(|_| io::Error::other("PDF stream length overflow"))?,
            );
            write_pdf_dictionary(output, &dictionary)?;
            output.write_all(b"\nstream\n")?;
            output.write_all(&stream.content)?;
            output.write_all(b"\nendstream")
        }
        Object::Reference((number, generation)) => write!(output, "{number} {generation} R"),
    }
}

fn write_pdf_name(output: &mut impl Write, name: &[u8]) -> io::Result<()> {
    output.write_all(b"/")?;
    for &byte in name {
        if b" \t\n\r\x0C()<>[]{}/%#".contains(&byte) || !(33..=126).contains(&byte) {
            write!(output, "#{byte:02X}")?;
        } else {
            output.write_all(&[byte])?;
        }
    }
    Ok(())
}

fn write_pdf_string(
    output: &mut impl Write,
    value: &[u8],
    format: &StringFormat,
) -> io::Result<()> {
    match format {
        StringFormat::Literal => {
            output.write_all(b"(")?;
            for &byte in value {
                match byte {
                    b'(' => output.write_all(b"\\(")?,
                    b')' => output.write_all(b"\\)")?,
                    b'\\' => output.write_all(b"\\\\")?,
                    b'\n' => output.write_all(b"\\n")?,
                    b'\r' => output.write_all(b"\\r")?,
                    b'\t' => output.write_all(b"\\t")?,
                    0x08 => output.write_all(b"\\b")?,
                    0x0c => output.write_all(b"\\f")?,
                    _ => output.write_all(&[byte])?,
                }
            }
            output.write_all(b")")
        }
        StringFormat::Hexadecimal => {
            output.write_all(b"<")?;
            for byte in value {
                write!(output, "{byte:02X}")?;
            }
            output.write_all(b">")
        }
    }
}

fn write_pdf_dictionary(output: &mut impl Write, dictionary: &Dictionary) -> io::Result<()> {
    output.write_all(b"<<")?;
    for (key, value) in dictionary {
        output.write_all(b" ")?;
        write_pdf_name(output, key)?;
        output.write_all(b" ")?;
        write_pdf_object(output, value)?;
    }
    output.write_all(b" >>")
}

fn incremental_trailer(
    base: &IncrementalExportBase,
    maximum_object_number: u32,
) -> Result<Dictionary, IncrementalExportError> {
    let mut trailer = base.trailer.clone();
    for key in [
        b"Type".as_slice(),
        b"W".as_slice(),
        b"Index".as_slice(),
        b"Length".as_slice(),
        b"Filter".as_slice(),
        b"DecodeParms".as_slice(),
        b"XRefStm".as_slice(),
    ] {
        trailer.remove(key);
    }
    let size = maximum_object_number
        .checked_add(1)
        .ok_or(IncrementalExportError::OffsetOverflow)?;
    let previous = i64::try_from(base.previous_xref_offset)
        .map_err(|_| IncrementalExportError::InvalidPreviousXrefOffset)?;
    trailer.set("Size", i64::from(size));
    trailer.set("Prev", previous);
    Ok(trailer)
}

fn validate_xref_offset(offset: u64) -> Result<(), IncrementalExportError> {
    if offset > MAX_CLASSIC_XREF_OFFSET {
        Err(IncrementalExportError::OffsetOverflow)
    } else {
        Ok(())
    }
}

fn check_cancelled(
    cancellation: &IncrementalExportCancellation,
) -> Result<(), IncrementalExportError> {
    if cancellation.is_cancelled() {
        Err(IncrementalExportError::Cancelled)
    } else {
        Ok(())
    }
}

fn is_sha256_fingerprint(value: &str) -> bool {
    value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn hex_digest(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn unique_sidecar_path(target: &Path, extension: &str) -> PathBuf {
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("translated.pdf");
    target.with_file_name(format!(
        ".{name}.{}.{}.{extension}",
        std::process::id(),
        counter
    ))
}

#[cfg(windows)]
fn replace_file_atomic(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file_atomic(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(unix)]
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_parent_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

fn io_error(operation: &'static str, path: &Path, error: io::Error) -> IncrementalExportError {
    IncrementalExportError::Io {
        operation,
        path: path.to_path_buf(),
        message: error.to_string(),
    }
}

struct CountingWriter<W> {
    inner: W,
    bytes_written: u64,
}

impl<W> CountingWriter<W> {
    fn new(inner: W, bytes_written: u64) -> Self {
        Self {
            inner,
            bytes_written,
        }
    }

    fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
}

impl<W: Write> Write for CountingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.bytes_written = self
            .bytes_written
            .checked_add(u64::try_from(written).unwrap_or(u64::MAX))
            .ok_or_else(|| io::Error::other("incremental export byte count overflow"))?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

struct TempFileGuard {
    path: PathBuf,
    armed: bool,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use lopdf::{Document, Object};
    use sha2::{Digest, Sha256};

    use super::{
        export_incremental_pdf_atomic, export_source_pdf_atomic, IncrementalExportBase,
        IncrementalExportCancellation, IncrementalExportError, SOURCE_COPY_BUFFER_BYTES,
    };
    use crate::pdf_v3::{object_delta::PdfObjectDelta, source_object::PdfSourceObjectStore};
    use crate::rosetta_jobs::formats::pdf::test_helpers::fixture_path;

    #[test]
    fn streams_a_valid_incremental_update_and_atomically_replaces_destination() {
        let directory = TestDirectory::new("round-trip");
        let source_path = fixture_path("simple-one-page.pdf");
        let source = fs::read(&source_path).expect("source bytes");
        let source_document = Document::load_mem(&source).expect("source document");
        let source_objects =
            PdfSourceObjectStore::open(&source_path).expect("lazy source object store");
        let base =
            IncrementalExportBase::from_source_object_store(fingerprint(&source), &source_objects)
                .expect("lazy export base");
        let info_id = source_document
            .trailer
            .get(b"Info")
            .and_then(Object::as_reference)
            .expect("fixture Info object");
        let mut info = source_document
            .get_object(info_id)
            .and_then(Object::as_dict)
            .cloned()
            .expect("fixture Info dictionary");
        info.set(
            "Producer",
            Object::string_literal("Rosetta incremental export"),
        );
        let delta = PdfObjectDelta::try_from_objects(
            BTreeMap::from([(info_id, Object::Dictionary(info))]),
            source_document.max_id,
        )
        .expect("Info delta");
        let destination = directory.path().join("translated.pdf");
        fs::write(&destination, b"previous complete export").expect("previous destination");

        let result = export_incremental_pdf_atomic(
            &source_path,
            &destination,
            &base,
            &delta,
            &IncrementalExportCancellation::default(),
        )
        .expect("incremental export");
        let output = fs::read(&destination).expect("output bytes");
        let output_document = Document::load_mem(&output).expect("output document");
        let producer = output_document
            .trailer
            .get(b"Info")
            .and_then(Object::as_reference)
            .and_then(|id| output_document.get_object(id))
            .and_then(Object::as_dict)
            .and_then(|dictionary| dictionary.get(b"Producer"))
            .and_then(Object::as_str)
            .expect("updated producer");

        assert_eq!(producer, b"Rosetta incremental export");
        assert_eq!(output_document.get_pages().len(), 1);
        assert_eq!(result.source_bytes, source.len() as u64);
        assert_eq!(result.output_bytes, output.len() as u64);
        assert_eq!(result.delta_object_count, 1);
        assert_eq!(result.source_copy_buffer_bytes, SOURCE_COPY_BUFFER_BYTES);
        assert!(result.appended_bytes > 0);
        assert!(output.starts_with(&source));
        assert!(!directory.has_sidecar("tmp"));
    }

    #[test]
    fn cancellation_and_source_identity_failure_preserve_existing_destination() {
        let directory = TestDirectory::new("failure-atomicity");
        let source_path = fixture_path("simple-one-page.pdf");
        let source = fs::read(&source_path).expect("source bytes");
        let source_document = Document::load_mem(&source).expect("source document");
        let base = IncrementalExportBase::from_document(
            format!("sha256:{}", "0".repeat(64)),
            source.len() as u64,
            &source_document,
        )
        .expect("export base");
        let delta_object_number = source_document.max_id + 1;
        let delta = PdfObjectDelta::try_from_objects(
            BTreeMap::from([(
                (delta_object_number, 0),
                Object::string_literal("unused delta"),
            )]),
            delta_object_number,
        )
        .expect("unused delta");
        let destination = directory.path().join("translated.pdf");
        let previous = b"previous complete export";
        fs::write(&destination, previous).expect("previous destination");

        let error = export_incremental_pdf_atomic(
            &source_path,
            &destination,
            &base,
            &delta,
            &IncrementalExportCancellation::default(),
        )
        .expect_err("source identity mismatch");
        assert!(matches!(
            error,
            IncrementalExportError::SourceFingerprintMismatch
        ));
        assert_eq!(fs::read(&destination).expect("destination"), previous);
        assert!(!directory.has_sidecar("tmp"));

        let cancellation = IncrementalExportCancellation::default();
        cancellation.cancel();
        let valid_base = IncrementalExportBase::from_document(
            fingerprint(&source),
            source.len() as u64,
            &source_document,
        )
        .expect("valid export base");
        let error = export_incremental_pdf_atomic(
            &source_path,
            &destination,
            &valid_base,
            &delta,
            &cancellation,
        )
        .expect_err("cancelled export");
        assert!(matches!(error, IncrementalExportError::Cancelled));
        assert_eq!(fs::read(&destination).expect("destination"), previous);
        assert!(!directory.has_sidecar("tmp"));
    }

    #[test]
    fn verified_source_copy_atomically_replaces_destination_without_appended_bytes() {
        let directory = TestDirectory::new("source-copy");
        let source_path = fixture_path("simple-one-page.pdf");
        let source = fs::read(&source_path).expect("source bytes");
        let source_objects =
            PdfSourceObjectStore::open(&source_path).expect("lazy source object store");
        let base =
            IncrementalExportBase::from_source_object_store(fingerprint(&source), &source_objects)
                .expect("source copy base");
        let destination = directory.path().join("preserved.pdf");
        fs::write(&destination, b"previous complete export").expect("previous destination");

        let result = export_source_pdf_atomic(
            &source_path,
            &destination,
            &base,
            &IncrementalExportCancellation::default(),
        )
        .expect("verified source copy");

        assert_eq!(fs::read(&destination).expect("copied PDF"), source);
        assert_eq!(result.source_bytes, source.len() as u64);
        assert_eq!(result.output_bytes, source.len() as u64);
        assert_eq!(result.source_copy_buffer_bytes, SOURCE_COPY_BUFFER_BYTES);
        assert!(!directory.has_sidecar("tmp"));
    }

    fn fingerprint(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!(
            "sha256:{}",
            hasher
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        )
    }

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rosetta-pdf-v3-incremental-export-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn has_sidecar(&self, extension: &str) -> bool {
            fs::read_dir(&self.0)
                .expect("list test directory")
                .filter_map(Result::ok)
                .any(|entry| {
                    entry
                        .path()
                        .extension()
                        .is_some_and(|value| value == extension)
                })
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
