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

- page-level state remains in `pdf_page_translations.json`
- translated page PDFs remain in `pdf-pages/page-000N.pdf`
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
