# 2026-06-24 PDF Preview UX Performance

## Context

PDF visual translation now renders source and translated pages as PDFium-rasterized PNG images. Users found four related issues in the PDF workflow:

- untranslated / translating PDF page states looked visually rough
- translated pages flashed when a later page finished translating
- scrolling became very slow during those updates
- switching between already translated ~15 MB PDFs forced source and translated pages to visibly reload

The root cause is mostly frontend lifecycle behavior rather than the PDF translation model itself. The translated pane used a global cache key that remounted every translated page on each page-progress event, and the PDF pane mounted every page at once. Switching documents also discarded the visible PDF preview before the newly selected job had reloaded.

## Scope

This change keeps the existing visual PDF translation architecture:

- page-level state remains JSON-based under the job cache
- translated page PDFs remain page-level artifacts under `pdf-pages/`
- preview continues to use backend PDFium rasterization through narrow Tauri commands
- export continues assembling source pages with translated page PDFs

It does not add OCR, chat, cloud upload, text selection in PDF preview, bilingual PDF export, or a new PDF data model.

## Implementation Plan

- Make PDF page rendering page-scoped:
  - remove the global translated cache key from normal progress updates
  - keep page row keys stable across unrelated page status changes
  - reload only the translated page whose underlying page PDF changed
  - preserve the existing image while a replacement image loads
- Virtualize PDF page panes with `@tanstack/react-virtual`:
  - mount only visible and nearby source / translated pages
  - keep source and translated rows aligned through a shared page-height estimate
  - keep page controls and status UI inside the virtualized rows
- Update page state incrementally:
  - load the full page state on initial job / target-language entry
  - patch only the affected page on `rosetta-pdf-page-progress`
  - do a full refresh after translation completes to recover from any missed event
- Improve PDF page status UI:
  - use static, restrained placeholders for untranslated and queued pages
  - animate only the active translating page
  - show failed pages with compact error copy
  - keep completed pages visually quiet and stable
- Add a small frontend PDF preview cache:
  - remember recent page state, page count, visible page images, and scroll offsets by job / language
  - show cached content immediately when switching back to a recently viewed PDF
  - keep the cache in memory only and bounded to avoid holding many large PNGs

## Validation

Relevant commands:

```powershell
cd rosetta-app
pnpm typecheck
cd src-tauri
cargo check
cargo test rosetta_jobs
```

Manual checks:

- Translate multiple PDF pages and confirm previously translated pages do not flash when another page completes.
- Scroll while PDF pages are translating and confirm scrolling remains responsive.
- Confirm source / translation scroll sync still aligns by page.
- Switch between two already translated PDFs and confirm the second visit shows cached pages quickly while refreshing in the background.
- Confirm untranslated, queued, translating, failed, and translated states remain readable in light and dark themes.

## Follow-up: Target-language Isolation

After the preview performance change, a Windows test revealed that a PDF translated with target `en` could still display Chinese pages. The RWKV debug log showed the model request was correct (`Chinese: ... English:`) and returned English text, so the issue was page-cache reuse rather than translation direction.

Root cause:

- `pdf_page_translations.json` was shared across target languages.
- Generated page PDFs were shared at `pdf-pages/page-000N.pdf`.
- Reading page state overwrote `state.targetLang` with the requested target language, so stale pages could appear already translated for a different language.

Fix:

- New page-state writes use `pdf_page_translations.<targetLang>.json`.
- New page artifacts use `pdf-pages/<targetLang>/page-000N.pdf`.
- Legacy `pdf_page_translations.json` and `pdf-pages/page-000N.pdf` remain readable for compatibility, but language mismatches return an empty state and non-Chinese targets do not trust legacy shared page paths.
- The translated-page rasterization command now receives the current target language and resolves the page artifact through that language's page state.

## Follow-up: Long PDF Text Blocks

A new 8-page earnings-release PDF exposed another PDF-specific failure mode. The page-level visual translation path sends text blocks extracted by pdf2zh through Rosetta's local OpenAI shim, not through Rosetta's normal document segmenter. pdf2zh can merge several financial bullet paragraphs into one request; with the small-context llama.cpp profile this produced repeated provider errors like:

```txt
request (393 tokens) exceeds the available context size (256 tokens)
```

Because the error happened inside pdf2zh's worker loop, the UI looked like it was stuck on the first page while the same impossible request was retried many times.

Fix:

- The OpenAI shim now splits long pdf2zh source text into conservative sentence/word chunks before sending it to RWKV, then joins the chunk translations before responding to pdf2zh.
- llama.cpp provider errors now surface the JSON error message, instead of only reporting `HTTP 400`.
- The persistent pdf2zh worker now treats repeated identical RWKV errors as an unrecoverable run failure and restarts the worker, so page state moves to failed instead of appearing to run forever.

This does not change the normal TXT/Markdown/DOCX segmentation path. It is a guardrail for the visual PDF path where pdf2zh owns text extraction and calls the shim directly.
