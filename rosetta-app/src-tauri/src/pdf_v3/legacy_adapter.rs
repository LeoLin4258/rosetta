use std::fmt;

use super::{
    paragraph_translation_plan::{build_visual_paragraph_page_plan, VisualParagraphPlanError},
    types::{PageGraph, PAGE_GRAPH_SCHEMA_VERSION},
};

pub(crate) const LEGACY_ADAPTER_MAX_PAGE_SOURCE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const LEGACY_ADAPTER_MAX_UNITS_PER_PAGE: usize = 25_000;
pub(crate) const LEGACY_ADAPTER_DEFAULT_MERGED_SOURCE_CHARS: usize = 800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyPdfUnitCandidate {
    pub unit_id: String,
    pub page_number: u32,
    pub order_on_page: u32,
    pub paragraph_group_id: String,
    pub flow_container_group_id: String,
    pub source_text: String,
    pub provider_text: String,
    pub source_chars: u64,
    pub kind: String,
    pub requires_translation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LegacyPdfPageUnitWindow {
    pub page_number: u32,
    pub source_page_hash: String,
    pub units: Vec<LegacyPdfUnitCandidate>,
    pub preserved_container_count: usize,
    pub source_chars: u64,
}

#[derive(Debug)]
pub(crate) enum LegacyPdfAdapterError {
    InvalidPageGraph(&'static str),
    SourceBudgetExceeded { bytes: usize, maximum: usize },
    UnitBudgetExceeded { units: usize, maximum: usize },
    Plan(String),
}

impl fmt::Display for LegacyPdfAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPageGraph(field) => {
                write!(
                    formatter,
                    "legacy PDF adapter received invalid PageGraph {field}"
                )
            }
            Self::SourceBudgetExceeded { bytes, maximum } => write!(
                formatter,
                "legacy PDF adapter page source is {bytes} bytes, above {maximum}-byte limit"
            ),
            Self::UnitBudgetExceeded { units, maximum } => write!(
                formatter,
                "legacy PDF adapter page has {units} units, above {maximum}-unit limit"
            ),
            Self::Plan(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for LegacyPdfAdapterError {}

impl From<VisualParagraphPlanError> for LegacyPdfAdapterError {
    fn from(value: VisualParagraphPlanError) -> Self {
        Self::Plan(value.to_string())
    }
}

pub(crate) fn build_legacy_page_unit_window(
    page: &PageGraph,
) -> Result<LegacyPdfPageUnitWindow, LegacyPdfAdapterError> {
    if page.schema_version != PAGE_GRAPH_SCHEMA_VERSION
        || page.page_number == 0
        || page.source_page_hash.is_empty()
    {
        return Err(LegacyPdfAdapterError::InvalidPageGraph("identity"));
    }

    let source_bytes = page
        .atoms
        .iter()
        .map(|atom| atom.source_text.len())
        .try_fold(0usize, |total, bytes| total.checked_add(bytes))
        .ok_or(LegacyPdfAdapterError::SourceBudgetExceeded {
            bytes: usize::MAX,
            maximum: LEGACY_ADAPTER_MAX_PAGE_SOURCE_BYTES,
        })?;
    if source_bytes > LEGACY_ADAPTER_MAX_PAGE_SOURCE_BYTES {
        return Err(LegacyPdfAdapterError::SourceBudgetExceeded {
            bytes: source_bytes,
            maximum: LEGACY_ADAPTER_MAX_PAGE_SOURCE_BYTES,
        });
    }

    let plan = build_visual_paragraph_page_plan(page)?;
    if plan.units.len() > LEGACY_ADAPTER_MAX_UNITS_PER_PAGE {
        return Err(LegacyPdfAdapterError::UnitBudgetExceeded {
            units: plan.units.len(),
            maximum: LEGACY_ADAPTER_MAX_UNITS_PER_PAGE,
        });
    }

    let units = plan
        .units
        .into_iter()
        .map(|unit| LegacyPdfUnitCandidate {
            unit_id: unit.unit_id,
            page_number: page.page_number,
            order_on_page: unit.order_on_page,
            paragraph_group_id: unit.paragraph_group_id,
            flow_container_group_id: unit.flow_container_group_id,
            source_chars: unit.source_text.chars().count() as u64,
            source_text: unit.source_text,
            provider_text: unit.provider_text,
            kind: "paragraph".to_string(),
            requires_translation: true,
        })
        .collect::<Vec<_>>();
    let source_chars = units.iter().map(|unit| unit.source_chars).sum();

    Ok(LegacyPdfPageUnitWindow {
        page_number: page.page_number,
        source_page_hash: page.source_page_hash.clone(),
        units,
        preserved_container_count: plan.preserved_containers.len(),
        source_chars,
    })
}

pub(crate) fn merge_legacy_page_unit_window(
    window: LegacyPdfPageUnitWindow,
    max_source_chars: usize,
) -> Result<LegacyPdfPageUnitWindow, LegacyPdfAdapterError> {
    if max_source_chars == 0 {
        return Err(LegacyPdfAdapterError::InvalidPageGraph("mergeBudget"));
    }
    let mut merged = Vec::with_capacity(window.units.len());
    for unit in window.units {
        let can_merge = merged
            .last()
            .is_some_and(|previous: &LegacyPdfUnitCandidate| {
                previous.flow_container_group_id == unit.flow_container_group_id
                    && previous
                        .source_chars
                        .saturating_add(unit.source_chars)
                        .saturating_add(1)
                        <= max_source_chars as u64
            });
        if can_merge {
            let previous = merged.last_mut().expect("merge candidate");
            previous.source_text.push('\n');
            previous.source_text.push_str(&unit.source_text);
            previous.provider_text.push('\n');
            previous.provider_text.push_str(&unit.provider_text);
            previous.source_chars = previous
                .source_chars
                .saturating_add(unit.source_chars)
                .saturating_add(1);
        } else {
            merged.push(unit);
        }
    }
    let source_chars = merged.iter().map(|unit| unit.source_chars).sum();
    Ok(LegacyPdfPageUnitWindow {
        units: merged,
        source_chars,
        ..window
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, time::Instant};

    use windows_sys::Win32::System::{
        ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
        },
        Threading::GetCurrentProcess,
    };

    use super::{
        build_legacy_page_unit_window, merge_legacy_page_unit_window,
        LEGACY_ADAPTER_DEFAULT_MERGED_SOURCE_CHARS,
    };
    use crate::pdf_v3::{
        document::DocumentHandle,
        mapping::PageOperandMappingIndex,
        page_graph_store::PageGraphStore,
        page_set::PageSet,
        reconcile::{
            build_reconciled_page_graph_from_handle,
            build_reconciled_page_graph_from_handle_with_index,
        },
        types::PDF_V3_ENGINE_VERSION,
    };
    use crate::rosetta_jobs::formats::pdf::test_helpers::{
        fixture_path, pdfium_test_lock, shared_pdfium,
    };

    #[test]
    fn builds_one_bounded_legacy_unit_window_from_one_page() {
        let _guard = pdfium_test_lock();
        let source = fixture_path("002-trivial-libre-office-writer.pdf");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let page = build_reconciled_page_graph_from_handle(&handle, 1).expect("PageGraph");
        let window = build_legacy_page_unit_window(&page).expect("legacy unit window");

        assert_eq!(window.page_number, 1);
        assert!(!window.source_page_hash.is_empty());
        assert!(!window.units.is_empty());
        assert!(window.units.iter().all(|unit| unit.page_number == 1));
        assert!(window
            .units
            .windows(2)
            .all(|pair| { pair[0].order_on_page <= pair[1].order_on_page }));
        assert!(window.source_chars > 0);
    }

    #[derive(Debug, Clone, Copy)]
    struct ProcessMemorySample {
        working_set_bytes: usize,
        peak_working_set_bytes: usize,
        private_bytes: usize,
    }

    fn process_memory_sample() -> ProcessMemorySample {
        let mut counters = unsafe { std::mem::zeroed::<PROCESS_MEMORY_COUNTERS_EX>() };
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let result = unsafe {
            K32GetProcessMemoryInfo(
                GetCurrentProcess(),
                (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX)
                    .cast::<PROCESS_MEMORY_COUNTERS>(),
                counters.cb,
            )
        };
        assert_ne!(result, 0, "read Windows process memory counters");
        ProcessMemorySample {
            working_set_bytes: counters.WorkingSetSize,
            peak_working_set_bytes: counters.PeakWorkingSetSize,
            private_bytes: counters.PrivateUsage,
        }
    }

    fn directory_file_bytes(path: &std::path::Path) -> u64 {
        fs::read_dir(path)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .map(|path| {
                if path.is_dir() {
                    directory_file_bytes(&path)
                } else {
                    fs::metadata(path)
                        .map(|metadata| metadata.len())
                        .unwrap_or(0)
                }
            })
            .sum()
    }

    #[test]
    #[ignore = "manual Windows native preparse bounded-memory probe"]
    fn manual_windows_native_preparse_bounded_memory_probe() {
        let _guard = pdfium_test_lock();
        let source = std::env::var_os("ROSETTA_NATIVE_PREPARSE_SOURCE")
            .map(std::path::PathBuf::from)
            .expect("ROSETTA_NATIVE_PREPARSE_SOURCE");
        let handle = DocumentHandle::open(shared_pdfium(), &source).expect("document handle");
        let pages = PageSet::all(handle.page_count()).expect("all pages");
        let index = PageOperandMappingIndex::resolve(&handle, &pages).expect("mapping index");
        let root = std::env::temp_dir().join(format!(
            "rosetta-native-preparse-adapter-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("probe root");
        let store = PageGraphStore::new(
            &root,
            handle.source_fingerprint(),
            handle.page_count(),
            PDF_V3_ENGINE_VERSION,
        )
        .expect("PageGraph store");
        let started = Instant::now();
        let baseline = process_memory_sample();
        let mut max_working_set = baseline.working_set_bytes;
        let mut max_private = baseline.private_bytes;
        let mut max_units = 0usize;
        let mut max_merged_units = 0usize;
        let mut total_units = 0usize;
        let mut total_merged_units = 0usize;
        let mut total_source_chars = 0u64;
        let mut committed_pages = 0u32;

        for page_number in pages.pages() {
            let page =
                build_reconciled_page_graph_from_handle_with_index(&handle, &index, *page_number)
                    .expect("reconcile page");
            store.commit(&page).expect("commit PageGraph");
            let window = build_legacy_page_unit_window(&page).expect("legacy unit window");
            let merged_window = merge_legacy_page_unit_window(
                window.clone(),
                LEGACY_ADAPTER_DEFAULT_MERGED_SOURCE_CHARS,
            )
            .expect("merged legacy unit window");
            max_units = max_units.max(window.units.len());
            max_merged_units = max_merged_units.max(merged_window.units.len());
            total_units = total_units.saturating_add(window.units.len());
            total_merged_units = total_merged_units.saturating_add(merged_window.units.len());
            total_source_chars = total_source_chars.saturating_add(window.source_chars);
            committed_pages = committed_pages.saturating_add(1);
            let memory = process_memory_sample();
            max_working_set = max_working_set.max(memory.working_set_bytes);
            max_private = max_private.max(memory.private_bytes);
            drop(window);
            drop(merged_window);
            drop(page);
        }

        let final_memory = process_memory_sample();
        println!(
            "pdf-v3 native-preparse-adapter pages={} units={} mergedUnits={} sourceChars={} elapsedMs={} maxUnitsPerPage={} maxMergedUnitsPerPage={} pageGraphDiskBytes={} baselineWorkingSet={} maxWorkingSet={} finalWorkingSet={} processPeakWorkingSet={} baselinePrivate={} maxPrivate={} finalPrivate={}",
            committed_pages,
            total_units,
            total_merged_units,
            total_source_chars,
            started.elapsed().as_millis(),
            max_units,
            max_merged_units,
            directory_file_bytes(&root),
            baseline.working_set_bytes,
            max_working_set,
            final_memory.working_set_bytes,
            final_memory.peak_working_set_bytes,
            baseline.private_bytes,
            max_private,
            final_memory.private_bytes,
        );
        let _ = fs::remove_dir_all(root);
    }
}
