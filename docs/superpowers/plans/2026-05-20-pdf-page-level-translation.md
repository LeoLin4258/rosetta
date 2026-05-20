# PDF Page-Level Translation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build PDF page selection, page-by-page visual translation preview, and complete-PDF export that preserves untranslated source pages.

**Architecture:** Add a PDF page state layer in Rust, then expose narrow Tauri commands for status, page translation, page rendering, cancellation, and export. The frontend keeps the existing PDF workbench but adds synchronized range input and per-page checkboxes, rendering translated page caches as they complete.

**Tech Stack:** Tauri v2, Rust, pdf2zh CLI `--pages`, pdfium-render rasterization, React, TypeScript, Tailwind/shadcn UI.

---

### Task 1: Page Selection And Page State

**Files:**
- Create: `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/page_state.rs`
- Modify: `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/mod.rs`
- Modify: `rosetta-app/src-tauri/src/rosetta_jobs/tests.rs`

- [ ] **Step 1: Write failing parser and state tests**

Add tests to `rosetta-app/src-tauri/src/rosetta_jobs/tests.rs`:

```rust
#[test]
fn pdf_page_selection_accepts_ranges_and_dedupes() {
    let pages = parse_pdf_page_selection("1-3, 3,5", 5).expect("valid selection");
    assert_eq!(pages, vec![1, 2, 3, 5]);
}

#[test]
fn pdf_page_selection_rejects_out_of_range_pages() {
    let error = parse_pdf_page_selection("2,6", 5).expect_err("page 6 is invalid");
    assert!(error.contains("第 6 页超出范围"));
}

#[test]
fn pdf_page_status_restores_stale_translating_pages() {
    let dir = unique_temp_dir("pdf-page-state");
    fs::create_dir_all(&dir).expect("create temp dir");
    let state = PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count: 2,
        target_lang: "zh-CN".to_string(),
        pages: vec![PdfPageTranslation {
            page_number: 1,
            status: "translating".to_string(),
            translated_pdf_path: None,
            error: None,
            updated_at: "1".to_string(),
        }],
    };
    write_pdf_page_translation_state(&dir, &state).expect("write state");

    let restored = read_pdf_page_translation_state(&dir, 2, "zh-CN").expect("read state");

    assert_eq!(restored.pages[0].status, "pending");
    fs::remove_dir_all(dir).ok();
}
```

- [ ] **Step 2: Run tests and confirm RED**

Run:

```bash
cd rosetta-app/src-tauri
cargo test pdf_page_selection_accepts_ranges_and_dedupes pdf_page_selection_rejects_out_of_range_pages pdf_page_status_restores_stale_translating_pages
```

Expected: compile fails because the page state functions and types do not exist.

- [ ] **Step 3: Implement minimal page state module**

Create `page_state.rs` with:

```rust
use std::{collections::BTreeSet, path::Path};

use serde::{Deserialize, Serialize};

use crate::rosetta_jobs::{model::SCHEMA_VERSION, path::timestamp_ms_string, store::{read_json, write_json}};

pub(crate) const PDF_PAGE_TRANSLATIONS_FILENAME: &str = "pdf_page_translations.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfPageTranslationState {
    pub schema_version: u32,
    pub source_page_count: u32,
    pub target_lang: String,
    pub pages: Vec<PdfPageTranslation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PdfPageTranslation {
    pub page_number: u32,
    pub status: String,
    pub translated_pdf_path: Option<String>,
    pub error: Option<String>,
    pub updated_at: String,
}

pub(crate) fn parse_pdf_page_selection(input: &str, source_page_count: u32) -> Result<Vec<u32>, String> {
    if source_page_count == 0 {
        return Err("PDF 没有可选择的页面。".to_string());
    }
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("请输入要翻译的页码。".to_string());
    }
    let mut pages = BTreeSet::new();
    for raw_part in trimmed.split(',') {
        let part = raw_part.trim();
        if part.is_empty() {
            return Err("页码范围里有空项。".to_string());
        }
        if let Some((start, end)) = part.split_once('-') {
            let start = parse_page_number(start.trim(), source_page_count)?;
            let end = parse_page_number(end.trim(), source_page_count)?;
            if start > end {
                return Err(format!("页码范围 {start}-{end} 的起始页不能大于结束页。"));
            }
            for page in start..=end {
                pages.insert(page);
            }
        } else {
            pages.insert(parse_page_number(part, source_page_count)?);
        }
    }
    Ok(pages.into_iter().collect())
}

pub(crate) fn read_pdf_page_translation_state(
    job_dir: &Path,
    source_page_count: u32,
    target_lang: &str,
) -> Result<PdfPageTranslationState, String> {
    let path = job_dir.join(PDF_PAGE_TRANSLATIONS_FILENAME);
    if !path.is_file() {
        return Ok(empty_state(source_page_count, target_lang));
    }
    let mut state: PdfPageTranslationState = read_json(&path)?;
    state.source_page_count = source_page_count;
    state.target_lang = target_lang.to_string();
    for page in &mut state.pages {
        if page.status == "translating" || page.status == "queued" {
            page.status = "pending".to_string();
            page.updated_at = timestamp_ms_string();
        }
    }
    Ok(state)
}

pub(crate) fn write_pdf_page_translation_state(
    job_dir: &Path,
    state: &PdfPageTranslationState,
) -> Result<(), String> {
    write_json(&job_dir.join(PDF_PAGE_TRANSLATIONS_FILENAME), state)
}

pub(crate) fn upsert_pdf_page(
    state: &mut PdfPageTranslationState,
    page_number: u32,
    status: &str,
    translated_pdf_path: Option<String>,
    error: Option<String>,
) {
    let updated_at = timestamp_ms_string();
    if let Some(page) = state.pages.iter_mut().find(|page| page.page_number == page_number) {
        page.status = status.to_string();
        page.translated_pdf_path = translated_pdf_path;
        page.error = error;
        page.updated_at = updated_at;
        return;
    }
    state.pages.push(PdfPageTranslation {
        page_number,
        status: status.to_string(),
        translated_pdf_path,
        error,
        updated_at,
    });
    state.pages.sort_by_key(|page| page.page_number);
}

pub(crate) fn empty_state(source_page_count: u32, target_lang: &str) -> PdfPageTranslationState {
    PdfPageTranslationState {
        schema_version: SCHEMA_VERSION,
        source_page_count,
        target_lang: target_lang.to_string(),
        pages: Vec::new(),
    }
}

fn parse_page_number(input: &str, source_page_count: u32) -> Result<u32, String> {
    let page = input
        .parse::<u32>()
        .map_err(|_| format!("页码 `{input}` 不是有效数字。"))?;
    if page == 0 {
        return Err("页码必须从 1 开始。".to_string());
    }
    if page > source_page_count {
        return Err(format!("第 {page} 页超出范围，当前 PDF 共 {source_page_count} 页。"));
    }
    Ok(page)
}
```

Export it from `formats/pdf/mod.rs`:

```rust
pub(crate) mod page_state;
```

Update test imports in `tests.rs`:

```rust
use crate::rosetta_jobs::formats::pdf::page_state::*;
```

- [ ] **Step 4: Run tests and confirm GREEN**

Run the same `cargo test` command. Expected: all three tests pass.

### Task 2: Page-Level PDF Commands

**Files:**
- Modify: `rosetta-app/src-tauri/src/rosetta_jobs/store.rs`
- Modify: `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs`
- Modify: `rosetta-app/src-tauri/src/rosetta_jobs/mod.rs`
- Modify: `rosetta-app/src-tauri/src/lib.rs`
- Modify: `rosetta-app/src/lib/rosettaJobs.ts`

- [ ] **Step 1: Add backend command tests where pure functions are available**

Add a test for page artifact path safety in `tests.rs` after Task 1:

```rust
#[test]
fn pdf_page_artifact_path_is_stable() {
    assert_eq!(pdf_page_filename(1), "page-0001.pdf");
    assert_eq!(pdf_page_filename(42), "page-0042.pdf");
}
```

- [ ] **Step 2: Run test and confirm RED**

Run:

```bash
cd rosetta-app/src-tauri
cargo test pdf_page_artifact_path_is_stable
```

Expected: compile fails because `pdf_page_filename` does not exist.

- [ ] **Step 3: Implement page paths, page status command, render command, and translation loop**

Add to `page_state.rs`:

```rust
pub(crate) fn pdf_page_filename(page_number: u32) -> String {
    format!("page-{page_number:04}.pdf")
}

pub(crate) fn pdf_page_relative_path(page_number: u32) -> String {
    format!("pdf-pages/{}", pdf_page_filename(page_number))
}
```

Add page filtering to `Pdf2zhInvokeOptions`:

```rust
pub pages: Option<Vec<u32>>,
```

When building the `pdf2zh` command, add:

```rust
if let Some(pages) = &options.pages {
    let pages_arg = pages.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    command.arg("--pages").arg(pages_arg);
}
```

Add Tauri commands in `rosetta_jobs/mod.rs` with these signatures:

```rust
#[tauri::command]
pub fn get_rosetta_pdf_page_status(
    app: AppHandle,
    job_id: String,
    target_lang: Option<String>,
) -> Result<formats::pdf::page_state::PdfPageTranslationState, String>;

#[tauri::command]
pub async fn translate_rosetta_pdf_pages(
    app: AppHandle,
    cancel_state: State<'_, PdfTranslationCancelState>,
    job_id: String,
    page_selection: String,
    target_lang: String,
    rwkv_base_url: String,
    source_lang: Option<String>,
    timeout_ms: Option<u64>,
    force: Option<bool>,
) -> Result<formats::pdf::page_state::PdfPageTranslationState, String>;

#[tauri::command]
pub fn render_rosetta_pdf_translated_page_as_png(
    app: AppHandle,
    job_id: String,
    page_number: u32,
    target_width: u32,
) -> Result<Vec<u8>, String>;
```

The implementation must:

- load job bundle and source page count;
- parse page selection;
- skip `translated` pages unless `force == Some(true)`;
- call `invoke_pdf2zh` with `pages: Some(vec![page])`;
- copy the one-page output to `<job>/pdf-pages/page-000N.pdf`;
- mark each page `translated` or `failed`;
- emit existing progress plus page-level state after each page.

Register the new commands in `src-tauri/src/lib.rs`.

Add TypeScript wrappers in `rosetta-app/src/lib/rosettaJobs.ts`.

- [ ] **Step 4: Run backend type check**

Run:

```bash
cd rosetta-app/src-tauri
cargo check
```

Expected: pass.

### Task 3: Frontend Page Selection Controls

**Files:**
- Modify: `rosetta-app/src/features/preview/PdfDocumentPreview.tsx`
- Modify: `rosetta-app/src/features/preview/PdfPane.tsx`
- Modify: `rosetta-app/src/features/workspace/WorkspacePage.tsx`
- Modify: `rosetta-app/src/lib/rosettaJobs.ts`

- [ ] **Step 1: Add TypeScript types and wrappers**

Add:

```ts
export type PdfPageTranslation = {
  pageNumber: number;
  status: "pending" | "queued" | "translating" | "translated" | "failed";
  translatedPdfPath?: string | null;
  error?: string | null;
  updatedAt: string;
};

export type PdfPageTranslationState = {
  schemaVersion: number;
  sourcePageCount: number;
  targetLang: string;
  pages: PdfPageTranslation[];
};
```

Add wrappers for the new commands.

- [ ] **Step 2: Implement synchronized selection UI**

In `PdfDocumentPreview`, add local state:

```ts
const [selectedPages, setSelectedPages] = useState<number[]>([]);
const [pageRangeInput, setPageRangeInput] = useState("");
const [forceRetranslate, setForceRetranslate] = useState(false);
const [pdfPageState, setPdfPageState] = useState<PdfPageTranslationState | null>(null);
```

Add helpers:

```ts
function formatPageSelection(pages: number[]) {
  const sorted = [...new Set(pages)].sort((a, b) => a - b);
  const ranges: string[] = [];
  let start = sorted[0];
  let previous = sorted[0];
  for (const page of sorted.slice(1)) {
    if (page === previous + 1) {
      previous = page;
      continue;
    }
    ranges.push(start === previous ? `${start}` : `${start}-${previous}`);
    start = page;
    previous = page;
  }
  if (start != null && previous != null) {
    ranges.push(start === previous ? `${start}` : `${start}-${previous}`);
  }
  return ranges.join(",");
}

function togglePage(page: number, checked: boolean) {
  setSelectedPages((current) => {
    const next = checked
      ? [...current, page]
      : current.filter((candidate) => candidate !== page);
    const normalized = [...new Set(next)].sort((a, b) => a - b);
    setPageRangeInput(formatPageSelection(normalized));
    return normalized;
  });
}
```

The header shows an input, a checkbox/toggle for retranslation, and a translate button.

- [ ] **Step 3: Add per-page controls to `PdfPane`**

Extend `PdfPane` props:

```ts
pageControls?: (pageIndex: number) => React.ReactNode;
pageStatusLabel?: (pageIndex: number) => React.ReactNode;
renderKindForPage?: (pageIndex: number) => "source" | "translated" | "translatedPage";
```

Render controls next to each page without resizing the page image.

- [ ] **Step 4: Wire WorkspacePage to page translation command**

For PDF jobs, call `translateRosettaPdfPages` instead of only whole-document generation when selected pages are present. Keep the old translate-all action as selecting `1-${sourcePageCount}`.

- [ ] **Step 5: Run frontend typecheck**

Run:

```bash
cd rosetta-app
pnpm typecheck
```

Expected: pass.

### Task 4: Complete PDF Export Assembly

**Files:**
- Create: `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/page_assemble.rs`
- Modify: `rosetta-app/src-tauri/Cargo.toml`
- Modify: `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/mod.rs`
- Modify: `rosetta-app/src-tauri/src/rosetta_jobs/mod.rs`
- Modify: `docs/engineering/change-log/2026-05-20-pdf-page-level-translation.md`

- [ ] **Step 1: Verify PDF assembly dependency**

Run:

```bash
cd rosetta-app/src-tauri
cargo tree -i pdfium-render
rg -n "new_pdf|save_to|copy_page|import_page|pages_mut" ~/.cargo/registry/src -g '*pdfium-render*' | head -40
```

Expected: if there is no safe page-copy and save API in `pdfium-render`, add `lopdf = "0.34"` to `Cargo.toml` and document that Rosetta uses `lopdf` only for PDF page assembly while keeping pdfium for rasterized preview.

- [ ] **Step 2: Write assembly tests**

Add tests that assemble a fixture with only page 2 translated and assert:

- output page count equals source page count;
- missing translated pages do not fail export.

- [ ] **Step 3: Implement assembly**

Build a complete PDF by copying pages from translated single-page PDFs when available, otherwise source pages.

- [ ] **Step 4: Replace PDF export command path**

Update `export_rosetta_translated_pdf` to assemble from page state before copying to the user target path.

- [ ] **Step 5: Run validation**

Run:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Expected: pass.

### Task 5: Polish And Documentation

**Files:**
- Modify: `docs/engineering/conventions/data-models.md`
- Modify: `docs/engineering/change-log/2026-05-20-pdf-page-level-translation.md`

- [ ] **Step 1: Update data model convention**

Document that visual PDF translation uses page-level PDF artifacts and does not populate text segments.

- [ ] **Step 2: Update change log**

Document commands, cache layout, validation, and the chosen PDF assembly dependency.

- [ ] **Step 3: Final verification**

Run:

```bash
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Expected: pass.
