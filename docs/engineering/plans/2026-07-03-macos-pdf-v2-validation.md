# macOS PDF v2 Validation Handoff

## Summary

This handoff asks a macOS agent to validate the PDF v2 progressive page
rendering path and build a fresh macOS PDF component pack before release
artifact upload.

Windows dogfooding has already validated the same v2 path:

- 10-page text PDF translates and refills correctly.
- Blank translated pages are no longer produced.
- Formula/rich-text placeholder mismatch is fixed.
- Non-Lightning translation commits pages progressively instead of waiting for
  the whole 10-page window to finish.

The macOS agent should verify that the same behavior holds on Apple Silicon
and report back with command results, pack metadata, and UI observations.

## Background

Recent PDF v2 changes added two important behaviors.

First, Rust protects PDF structural placeholders before model translation:

- PDF unit text is split so `{vN}` formula/rich-text placeholders are kept out
  of natural language model input.
- The provider translates only natural language chunks.
- Rust reconstructs the final unit text by putting the original placeholders
  back in order.
- The Python PDF engine still performs strict placeholder mismatch validation.

Second, PDF rendering is now progressive at page granularity:

- Rosetta still prepares a page window and feeds provider batches across the
  window to preserve RWKV batch efficiency.
- After each provider batch returns, Rust emits completed
  `unitId -> translation` events.
- When all translation units for one page are ready, Rust calls the persistent
  PDF worker to render only that page.
- Page commit still consumes formal `PageResult` values; diagnostics do not
  drive business control flow.

## Repositories

Use the local Rosetta and PDFMathTranslate checkouts.

Typical paths:

```txt
/path/to/rosetta
/path/to/PDFMathTranslate
```

If local paths differ, use the actual local paths and include them in the
report.

## Code Areas To Confirm

Rosetta app:

- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/unit_translation.rs`
  should contain `translate_pdf_units_with_events`,
  `emit_ready_unit_outputs`, and placeholder reconstruction tests.
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs`
  should receive unit completion events through a channel, group unit IDs by
  page, and call `render_ready_pages` as soon as a page is ready.
- `rosetta-app/src-tauri/src/managed_pdf2zh/rosetta_pdf2zh_worker.py`
  should pass `pages=job.get("pages")` into `engine.renderPages(...)`.

PDFMathTranslate fork:

- `pdf2zh/rosetta_engine.py` should expose
  `renderPages(..., pages: list[int] | None = None, ...)`.
- `renderPages` should call `normalize_render_pages(pages, state.pages)`.
- Partial render must validate only translation unit IDs for the selected
  render pages.
- `test/test_rosetta_engine.py` should include a selected-page render test
  similar to `test_render_selected_page_accepts_only_that_pages_translations`.

## Do Not

- Do not reintroduce the OpenAI-compatible shim or CLI fallback into the
  product PDF path.
- Do not split the model workload into one request per page just to make the
  UI look progressive.
- Do not loosen `PageResult` commit checks for empty translations, placeholder
  mismatch, unreadable artifacts, or missing artifacts.
- Do not write source text, translations, prompts, or raw model responses into
  diagnostics, timeline, profile, or logs.
- Do not upload release artifacts unless Leo explicitly asks.
- Do not make unrelated UI or architecture changes.

## Required Validation Commands

PDFMathTranslate fork:

```bash
cd /path/to/PDFMathTranslate

uv run python -m pytest test/test_rosetta_engine.py -q
python -m py_compile pdf2zh/rosetta_engine.py
```

If `uv` is unavailable, use the local project Python environment and mention
the substitution in the report.

Rosetta app:

```bash
cd /path/to/rosetta/rosetta-app

pnpm typecheck

cd src-tauri
cargo fmt -- --check
cargo check
cargo test unit_translation
cargo test pdf
cargo test rosetta_jobs
cargo test managed_pdf2zh
cargo test managed_rwkv
```

If any command fails because of macOS environment setup, include the exact
failure snippet. Do not summarize it as only "failed".

## Build macOS PDF Pack

On Apple Silicon macOS:

```bash
cd /path/to/rosetta/rosetta-app

bash src-tauri/scripts/build-pdf2zh-pack-macos-arm64.sh
```

Expected archive:

```txt
dist/pdf-layout/rosetta-pdf2zh-macos-arm64.tar.gz
```

Record the archive size and SHA256:

```bash
ls -lh dist/pdf-layout/
shasum -a 256 dist/pdf-layout/rosetta-pdf2zh-macos-arm64.tar.gz
```

If a manifest is generated, include its full contents in the report.

## Pack Content Check

After building, confirm the pack contains the updated Rosetta engine with
partial page rendering support:

```bash
cd /path/to/rosetta/rosetta-app

python3 - <<'PY'
import tarfile
from pathlib import Path

archive = Path("dist/pdf-layout/rosetta-pdf2zh-macos-arm64.tar.gz")
with tarfile.open(archive, "r:gz") as tf:
    names = [name for name in tf.getnames() if name.endswith("pdf2zh/rosetta_engine.py")]
    print(names)
    if not names:
        raise SystemExit("rosetta_engine.py not found in pack")
    data = tf.extractfile(names[0]).read().decode("utf-8")
    print("renderPages pages param:", "pages: list[int] | None = None" in data)
    print("normalize_render_pages:", "normalize_render_pages(pages, state.pages)" in data)
PY
```

Both checks should print `True`.

## UI Dogfood

Start the Rosetta dev app and import the newly built macOS PDF pack in
Settings. Manual UI interaction is acceptable; record the steps and observed
results.

Recommended test PDFs:

- A real text PDF around 10 pages.
- Prefer a PDF with normal body text plus formula/reference/rich-text
  placeholder cases.
- If time allows, also test a PDF under 30 pages.
- If time allows, test the first one or two windows of a PDF over 30 pages to
  confirm long-PDF responsiveness. Full translation of hundreds of pages is
  not required for this validation.

Observe and report:

- Whether the PDF component shows ready.
- Worker cold start / prewarm time.
- Whether the first translated page appears before the full window finishes.
- Whether progress increments progressively, for example `1/10`, `2/10`, and
  so on, instead of staying at `0/10` until the end.
- Whether translated pages are blank.
- Whether `formula placeholder mismatch` appears.
- Whether worker crash or render failure appears.
- Whether page artifacts are readable.
- Whether exported PDF page count is correct.
- Any artifact size or compression observations.

Provider expectations:

- For llama.cpp / non-Lightning, page-level progressive commit should be
  visible because provider batches return incrementally.
- For Lightning, if the provider returns one whole large batch and does not
  stream item-level results, the first page may still wait for that provider
  request to return. After the request returns, pages should render and commit
  quickly in order. This is not a regression unless Lightning itself supports
  item-level streaming and Rosetta is failing to use it.

## Report Template

Return the result in this format:

```markdown
# macOS PDF v2 Validation Report

## Environment

- macOS version:
- CPU/arch:
- Rosetta repo commit/branch:
- PDFMathTranslate repo commit/branch:
- Provider tested:
- Model/runtime:

## Pack

- Path:
- Size:
- SHA256:
- Manifest:
- Pack content check:
  - `renderPages(... pages ...)`: pass/fail
  - `normalize_render_pages`: pass/fail

## Command Validation

- `uv run python -m pytest test/test_rosetta_engine.py -q`: pass/fail
- `python -m py_compile pdf2zh/rosetta_engine.py`: pass/fail
- `pnpm typecheck`: pass/fail
- `cargo fmt -- --check`: pass/fail
- `cargo check`: pass/fail
- `cargo test unit_translation`: pass/fail
- `cargo test pdf`: pass/fail
- `cargo test rosetta_jobs`: pass/fail
- `cargo test managed_pdf2zh`: pass/fail
- `cargo test managed_rwkv`: pass/fail

Paste exact failure snippets for any failures.

## UI Dogfood

- Test PDF page count:
- Translation mode/provider:
- First translated page visible after:
- Progress behavior:
- Were pages committed progressively?
- Blank translated pages observed?
- Placeholder mismatch observed?
- Worker crash/render failure?
- Export page count correct?
- Artifact compression/size observations:

## Conclusion

- macOS pack is ready / not ready:
- Blocking issues:
- Non-blocking issues:
- Recommended next step:
```

## Acceptance Criteria

The macOS PDF component can proceed to release artifact preparation only if:

- PDFMathTranslate Rosetta engine tests pass.
- Rosetta `cargo check`, `cargo test pdf`, and `cargo test rosetta_jobs` pass.
- The macOS PDF pack builds successfully.
- The pack includes the updated `renderPages(..., pages=...)` engine.
- UI import of the pack shows the PDF component as ready.
- A real 10-page PDF translates and refills successfully.
- Non-Lightning translation displays committed pages progressively.
- No blank translated pages are observed.
- No new placeholder mismatch failures are observed.
- Failures, if any, enter explicit failed states instead of producing
  successful-looking blank artifacts.
