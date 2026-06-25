import { invoke } from "@tauri-apps/api/core";
import type {
  RosettaExportKind,
  RosettaExportResult,
  RosettaJobDeleteResult,
  RosettaJobFileDeleteResult,
  RosettaJobBundle,
  RosettaJobSummary,
  RosettaTranslationFileBundle,
  Segment,
  TranslationSegment,
  TranslationRevisionReason,
} from "../types/rosetta";

export type PdfPageTranslation = {
  pageNumber: number;
  status: "pending" | "queued" | "translating" | "translated" | "failed";
  translatedPdfPath?: string | null;
  artifactVersion?: string | null;
  error?: string | null;
  updatedAt: string;
  lastRunId?: string | null;
};

export type PdfPageTranslationState = {
  schemaVersion: number;
  sourcePageCount: number;
  targetLang: string;
  pages: PdfPageTranslation[];
};

export type PdfSourceMetadata = {
  schemaVersion: number;
  pageCount: number;
  sourceFingerprint: string;
  filename: string;
  originalPath?: string | null;
  importedAt: string;
  updatedAt: string;
};

export type PdfTranslationRunSnapshot = {
  schemaVersion: number;
  runId: string;
  jobId: string;
  targetLang: string;
  state: "idle" | "running" | "pausing" | "paused" | "failed" | "completed" | string;
  mode: "continue" | "retranslate-selected" | "retranslate-all" | string;
  requestedPages: number[];
  completedPages: number[];
  failedPages: number[];
  currentChunk: number[];
  ownerSessionId: string;
  leaseUpdatedAt: string;
  cancelRequested: boolean;
  startedAt: string;
  updatedAt: string;
  lastError?: string | null;
};

export type PdfJobSnapshot = {
  source?: PdfSourceMetadata | null;
  pages: PdfPageTranslationState;
  run?: PdfTranslationRunSnapshot | null;
  summary: {
    totalPages: number;
    completedPages: number;
    failedPages: number;
    pendingPages: number;
  };
  repairWarnings: string[];
};

export type PdfRepairResult = {
  jobId: string;
  repaired: boolean;
  recoverable: boolean;
  warnings: string[];
};

export type LocalDataResetItem = {
  label: string;
  path: string;
  deleted: boolean;
};

export type LocalDataResetResult = {
  items: LocalDataResetItem[];
  stoppedRuntime: boolean;
  cancelledRwkvInstall: boolean;
  cancelledPdf2zhInstall: boolean;
  cancelledPdfTranslation: boolean;
  runtimeStopError?: string | null;
};

export function createWelcomeDocument() {
  return invoke<RosettaJobBundle>("create_welcome_document");
}

export function createBlankTxtDocument(filename: string) {
  return invoke<RosettaJobBundle>("create_blank_txt_document", { filename });
}

export function updateTxtSourceFile(
  jobId: string,
  fileId: string,
  contents: string
) {
  return invoke<RosettaJobBundle>("update_txt_source_file", {
    jobId,
    fileId,
    contents,
  });
}

export function clearRosettaLocalData() {
  return invoke<LocalDataResetResult>("clear_rosetta_local_data");
}

export function importRosettaDocumentFromPath(path: string) {
  return invoke<RosettaJobBundle>("import_rosetta_document_from_path", { path });
}

export function importRosettaProjectFromDirectory(path: string) {
  return invoke<RosettaJobBundle>("import_rosetta_project_from_directory", {
    path,
  });
}

export function pickRosettaImportPath() {
  return invoke<string | null>("pick_rosetta_import_path");
}

export function pickRosettaImportDirectory() {
  return invoke<string | null>("pick_rosetta_import_directory");
}

export function pickRosettaExportPath(
  defaultFilename: string,
  format: "txt" | "markdown" | "pdf"
) {
  return invoke<string | null>("pick_rosetta_export_path", {
    defaultFilename,
    format,
  });
}

export function listRosettaJobs() {
  return invoke<RosettaJobSummary[]>("list_rosetta_jobs");
}

export function loadRosettaJob(jobId: string) {
  return invoke<RosettaJobBundle>("load_rosetta_job", { jobId });
}

export function saveRosettaSegments(jobId: string, segments: Segment[]) {
  return invoke<RosettaJobBundle>("save_rosetta_segments", {
    jobId,
    segments,
  });
}

export function ensureRosettaTranslationFile(
  jobId: string,
  sourceFileId: string,
  targetLang: string
) {
  return invoke<RosettaTranslationFileBundle>("ensure_rosetta_translation_file", {
    jobId,
    sourceFileId,
    targetLang,
  });
}

export function loadRosettaTranslationFile(
  jobId: string,
  translationFileId: string
) {
  return invoke<RosettaTranslationFileBundle>("load_rosetta_translation_file", {
    jobId,
    translationFileId,
  });
}

export function saveRosettaTranslationSegments(
  jobId: string,
  translationFileId: string,
  segments: TranslationSegment[]
) {
  return invoke<RosettaTranslationFileBundle>("save_rosetta_translation_segments", {
    jobId,
    translationFileId,
    segments,
  });
}

export function updateRosettaJobFileLanguages(
  jobId: string,
  fileId: string,
  sourceLang: string | null,
  targetLang: string
) {
  return invoke<RosettaJobBundle>("update_rosetta_job_file_languages", {
    jobId,
    fileId,
    sourceLang,
    targetLang,
  });
}

export function createRosettaTranslationRevision(
  jobId: string,
  fileId: string,
  reason: TranslationRevisionReason,
  scopeBlockIds?: string[] | null
) {
  return invoke<RosettaJobBundle>("create_rosetta_translation_revision", {
    jobId,
    fileId,
    reason,
    scopeBlockIds,
  });
}

export function renameRosettaJob(jobId: string, name: string) {
  return invoke<RosettaJobSummary[]>("rename_rosetta_job", {
    jobId,
    name,
  });
}

export function deleteRosettaJob(jobId: string) {
  return invoke<RosettaJobDeleteResult>("delete_rosetta_job", { jobId });
}

export function deleteRosettaJobFile(jobId: string, fileId: string) {
  return invoke<RosettaJobFileDeleteResult>("delete_rosetta_job_file", {
    jobId,
    fileId,
  });
}

export function exportRosettaJobFile(
  jobId: string,
  fileId: string,
  kind: RosettaExportKind,
  targetPath: string
) {
  return invoke<RosettaExportResult>("export_rosetta_job_file", {
    jobId,
    fileId,
    kind,
    targetPath,
  });
}

export function exportRosettaTranslationFile(
  jobId: string,
  translationFileId: string,
  kind: RosettaExportKind,
  targetPath: string
) {
  return invoke<RosettaExportResult>("export_rosetta_translation_file", {
    jobId,
    translationFileId,
    kind,
    targetPath,
  });
}

// ---- PDF preview / generation ----

export type RosettaPdfAssets = {
  sourcePdf: string;
  translatedPdf: string | null;
};

/// Resolve absolute filesystem paths. Useful for existence checks (e.g. "did
/// we generate a translated PDF yet?"). NOT a renderable URL — see
/// [`readRosettaPdfBytes`] for the actual bytes path.
export function getRosettaPdfAssets(jobId: string) {
  return invoke<RosettaPdfAssets>("get_rosetta_pdf_assets", { jobId });
}

/// Read a PDF file as raw bytes via Tauri IPC. Returned as `Uint8Array` ready
/// to hand to react-pdf via `<Document file={{ data }} />`.
///
/// Why bytes-over-IPC instead of asset:// URL: on macOS, WebKit refuses XHR
/// from the `tauri://localhost` webview origin to `asset://localhost/<path>`
/// (treats them as cross-protocol). The HTTP-aliased variant `http://localhost/<path>`
/// that `convertFileSrc(..., "http")` returns isn't routed to Tauri's asset
/// handler either. Pulling bytes through the existing IPC channel sidesteps
/// the whole URL/CORS dance.
export async function readRosettaPdfBytes(
  jobId: string,
  kind: "source" | "translated",
): Promise<Uint8Array> {
  // Tauri's binary IPC returns ArrayBuffer for `Response::new(Vec<u8>)`.
  const buffer = await invoke<ArrayBuffer>("read_rosetta_pdf_bytes", {
    jobId,
    kind,
  });
  return new Uint8Array(buffer);
}

/// Trigger the pdfium-based generate pipeline. Returns the absolute path of
/// the freshly-written translated PDF; the same path lives under
/// `<job_dir>/exports/translated.pdf` so subsequent `getRosettaPdfAssets`
/// calls see it as `translatedPdf`.
export function generateRosettaTranslatedPdf(
  jobId: string,
  options?: {
    rwkvBaseUrl?: string;
    providerId?: string;
    providerEndpoint?: string;
    providerInternalToken?: string;
    providerBodyPassword?: string;
    sourceLang?: string | null;
    targetLang?: string;
    timeoutMs?: number;
    ignoreCache?: boolean;
  },
) {
  return invoke<string>("generate_rosetta_translated_pdf", {
    jobId,
    rwkvBaseUrl: options?.rwkvBaseUrl,
    providerId: options?.providerId,
    providerEndpoint: options?.providerEndpoint,
    providerInternalToken: options?.providerInternalToken,
    providerBodyPassword: options?.providerBodyPassword,
    sourceLang: options?.sourceLang,
    targetLang: options?.targetLang,
    timeoutMs: options?.timeoutMs,
    ignoreCache: options?.ignoreCache,
  });
}

export function cancelRosettaTranslatedPdf() {
  return invoke<void>("cancel_rosetta_translated_pdf");
}

export function pauseRosettaPdfRun(
  jobId: string,
  targetLang: string,
  runId?: string | null,
) {
  return invoke<PdfTranslationRunSnapshot | null>("pause_rosetta_pdf_run", {
    jobId,
    targetLang,
    runId,
  });
}

export function repairRosettaPdfJob(jobId: string) {
  return invoke<PdfRepairResult>("repair_rosetta_pdf_job", { jobId });
}

export function getRosettaPdfSnapshot(
  jobId: string,
  targetLang?: string | null,
) {
  return invoke<PdfJobSnapshot>("get_rosetta_pdf_snapshot", {
    jobId,
    targetLang,
  });
}

export function getRosettaPdfPageStatus(
  jobId: string,
  targetLang?: string | null,
) {
  return invoke<PdfPageTranslationState>("get_rosetta_pdf_page_status", {
    jobId,
    targetLang,
  });
}

export function translateRosettaPdfPages(
  jobId: string,
  options: {
    pageSelection: string;
    targetLang: string;
    rwkvBaseUrl: string;
    providerId?: string;
    providerEndpoint?: string;
    providerInternalToken?: string;
    providerBodyPassword?: string;
    sourceLang?: string | null;
    timeoutMs?: number;
    force?: boolean;
  },
) {
  return invoke<PdfPageTranslationState>("translate_rosetta_pdf_pages", {
    jobId,
    pageSelection: options.pageSelection,
    targetLang: options.targetLang,
    rwkvBaseUrl: options.rwkvBaseUrl,
    providerId: options.providerId,
    providerEndpoint: options.providerEndpoint,
    providerInternalToken: options.providerInternalToken,
    providerBodyPassword: options.providerBodyPassword,
    sourceLang: options.sourceLang,
    timeoutMs: options.timeoutMs,
    force: options.force,
  });
}

/// Copy the cached translated PDF (`<job_dir>/exports/translated.pdf`) to a
/// user-chosen destination. Re-generation is unnecessary — the bytes on disk
/// are exactly the v1 pipeline output. PDF v1 doesn't support bilingual
/// side-by-side export.
export function exportRosettaTranslatedPdf(
  jobId: string,
  targetPath: string,
  targetLang?: string | null,
) {
  return invoke<RosettaExportResult>("export_rosetta_translated_pdf", {
    jobId,
    targetLang,
    targetPath,
  });
}

/// Page count of either the source or translated PDF. Returned synchronously
/// so the frontend can pre-allocate page placeholders before any pixels load.
export function countRosettaPdfPages(
  jobId: string,
  kind: "source" | "translated",
) {
  return invoke<number>("count_rosetta_pdf_pages", { jobId, kind });
}

/// Rasterize a single PDF page to PNG bytes on the backend. We do this
/// instead of feeding the PDF to pdfjs / `<embed>` because (a) pdfium's
/// per-page font subsets break pdfjs's @font-face loader (translated CJK
/// renders as gibberish in the webview even though Preview / sips render
/// the same PDF correctly), and (b) Tauri's WKWebView in app mode lacks
/// the PDF plugin Safari proper uses for `<embed>`. Rasterizing server-side
/// gives us identical output to Preview at the cost of text-selection in
/// the preview (the exported PDF still has it).
export async function renderRosettaPdfPageAsPng(
  jobId: string,
  kind: "source" | "translated",
  pageIndex: number,
  targetWidth: number,
): Promise<Uint8Array> {
  const buffer = await invoke<ArrayBuffer>("render_rosetta_pdf_page_as_png", {
    jobId,
    kind,
    pageIndex,
    targetWidth,
  });
  return new Uint8Array(buffer);
}

export async function renderRosettaPdfTranslatedPageAsPng(
  jobId: string,
  pageNumber: number,
  targetWidth: number,
  targetLang?: string | null,
): Promise<Uint8Array> {
  const buffer = await invoke<ArrayBuffer>(
    "render_rosetta_pdf_translated_page_as_png",
    {
      jobId,
      pageNumber,
      targetLang,
      targetWidth,
    },
  );
  return new Uint8Array(buffer);
}
