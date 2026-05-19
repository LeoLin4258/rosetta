# PDF Phase 3 pdf2zh Switch

## Context

PDF Phase 2 used Docling extraction plus an in-house pdfium write-back path. It worked for simple text PDFs, but complex layouts required Rosetta to own reflow, table fitting, formula protection, and font placement. Phase 3 switches PDF translation to PDFMathTranslate (`pdf2zh`) as an end-to-end black-box PDF workflow.

## Changes

- Added a `managed_pdf2zh` backend boundary with macOS Apple Silicon status detection and a local-dev `ROSETTA_PDF2ZH_BIN` binary override.
- Added an axum OpenAI-compatible shim that exposes `/v1/chat/completions` to pdf2zh and forwards requests to the local RWKV `/v1/batch/chat` provider.
- Replaced PDF import extraction with a lightweight pdfium pre-flight and cached source PDF skeleton.
- Replaced translated-PDF generation with a pdf2zh invocation that copies the generated translated PDF into the existing Rosetta translated PDF cache path. The current dogfood target is `pdf2zh 1.7.9`, whose CLI writes `*-zh.pdf`; the code also tolerates the planned/newer `*-mono.pdf` naming.
- Updated the workspace and PDF preview UI so PDF translation runs through the black-box pdf2zh path instead of segment-by-segment translation.
- Removed the Docling sidecar module, pdfium text extraction, pdfium PDF write-back generator, and the bundled Source Han font resource.
- Kept pdfium rasterization for source and translated PDF preview.

## Current Limitation

The downloadable pdf2zh pack installer is not implemented in this change. Development and dogfood runs should set `ROSETTA_PDF2ZH_BIN` to a local pdf2zh executable, or place the pack binary at the managed pack path reported by `get_pdf2zh_status`.

Current local dogfood command:

```bash
cd rosetta-app
ROSETTA_PDF2ZH_BIN="$PWD/.venv-pdf2zh/bin/pdf2zh" pnpm tauri dev
```

The local dogfood venv was created with:

```bash
python3 -m venv .venv-pdf2zh
. .venv-pdf2zh/bin/activate
pip install pdf2zh
```

For the current `pdf2zh 1.7.9` + Python 3.13 environment, `pdf2zh/high_level.py` needed a local compatibility patch:

```python
np.fromstring(pix.samples, np.uint8)
# -> 
np.frombuffer(pix.samples, np.uint8)
```

The real pack should avoid relying on this mutable local patch by either pinning a compatible Python/NumPy/pdf2zh set or applying the patch during pack construction.

## Dogfood Findings

- Importing PDF is fast and source preview appears immediately because import only performs pdfium pre-flight and caches `source.pdf`.
- The initial pdf2zh CLI invocation failed because the pip-installed `pdf2zh 1.7.9` does not support `-o`, `--openai-base-url`, `--openai-api-key`, or `--openai-model`. The invocation now uses `openai:rwkv`, environment variables, and `current_dir(output_dir)`.
- pdf2zh caches translations under `tempfile.gettempdir()/cache`. Repeated generation reused an earlier bad prompt-translation result and never called the shim. The subprocess now gets per-job `TMPDIR` / `TEMP` / `TMP` under `pdf2zh-output/tmp`, and Rosetta clears `pdf2zh-output` before each run.
- pdf2zh sends prompts shaped like `Source Text: ... Translated Text:`. The shim must extract only the source text before calling RWKV.
- Empty `Source Text` prompts must return an empty translation. Falling back to the full prompt leaks instructions such as "Translate the following markdown source text..." into the generated PDF.
- pdf2zh formula placeholders such as `$v0$` must pass through unchanged. Sending placeholder-only strings to RWKV can produce hallucinated explanatory text.
- The current shim logs `raw_user_preview`, `extracted_preview`, and `rwkv translation_preview` to both stderr and `pdf2zh-output/rosetta-pdf2zh-shim.log`. This is useful while dogfooding but should become debug-gated before release.
- Basic dogfood PDFs now complete end to end: import → source preview → pdf2zh/RWKV translation → translated PDF preview → exportable cached PDF. Remaining quality issues are mostly model/prompt/layout tradeoffs rather than broken plumbing.

## Next Steps

- Add a debug switch for shim logs so normal dev runs do not spam the terminal.
- Build the real macOS arm64 pdf2zh pack and replace the installer stubs.
- Lock or patch the pdf2zh environment used by the pack.
- Run a larger dogfood set: academic paper, simple office PDF, table-heavy PDF, scanned/image-only PDF.
- Decide whether to keep using `pdf2zh 1.7.9` temporarily or move to the newer PDFMathTranslate/BabelDOC CLI expected by the original plan.

## Validation

- `cd rosetta-app && pnpm typecheck`
- `cd rosetta-app/src-tauri && cargo check`
- `cd rosetta-app/src-tauri && cargo test rosetta_jobs`

`cargo check` and `cargo test rosetta_jobs` still report the pre-existing dead-code warning for `PdfError::Encrypted`.
