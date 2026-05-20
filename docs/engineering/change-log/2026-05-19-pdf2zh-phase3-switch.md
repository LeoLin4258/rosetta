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

The downloadable pdf2zh pack installer backend is now wired, but the app still does not ship an official pack URL or settings/onboarding UI for it. Development and dogfood runs can set `ROSETTA_PDF2ZH_BIN` to a local pdf2zh executable, place the pack binary at the managed pack path reported by `get_pdf2zh_status`, or invoke `install_pdf2zh_pack` with a `file://` / `https://` archive URL.

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

To stage the current dogfood environment into Rosetta's managed pack layout without using `ROSETTA_PDF2ZH_BIN`:

```bash
cd rosetta-app
bash src-tauri/scripts/stage-pdf2zh-pack-local.sh
pnpm tauri dev
```

This creates:

```txt
~/Library/Application Support/com.rosetta.desktop/pdf2zh-sidecar/pack/macos-arm64/
  python/      # local venv
  bin/pdf2zh   # relative wrapper
```

The script currently installs `pdf2zh==1.7.9` and applies the local NumPy compatibility patch.

The installer command now supports:

- `install_pdf2zh_pack({ options: { packUrl, packSha256, packSizeBytes, repair, proxyUrl } })`
- `ROSETTA_PDF2ZH_PACK_URL`, `ROSETTA_PDF2ZH_PACK_SHA256`, and `ROSETTA_PDF2ZH_PACK_SIZE_BYTES`
- progress snapshots/events via `get_pdf2zh_install_progress` and `managed-pdf2zh://install-progress`
- cancellation via `cancel_pdf2zh_install`

## Dogfood Findings

- Importing PDF is fast and source preview appears immediately because import only performs pdfium pre-flight and caches `source.pdf`.
- The initial pdf2zh CLI invocation failed because the pip-installed `pdf2zh 1.7.9` does not support `-o`, `--openai-base-url`, `--openai-api-key`, or `--openai-model`. The invocation now uses `openai:rwkv`, environment variables, and `current_dir(output_dir)`.
- pdf2zh caches translations under `tempfile.gettempdir()/cache`. Repeated generation reused an earlier bad prompt-translation result and never called the shim. The subprocess now gets per-job `TMPDIR` / `TEMP` / `TMP` under `pdf2zh-output/tmp`, and Rosetta clears `pdf2zh-output` before each run.
- pdf2zh sends prompts shaped like `Source Text: ... Translated Text:`. The shim must extract only the source text before calling RWKV.
- Empty `Source Text` prompts must return an empty translation. Falling back to the full prompt leaks instructions such as "Translate the following markdown source text..." into the generated PDF.
- pdf2zh formula placeholders such as `$v0$` must pass through unchanged. Sending placeholder-only strings to RWKV can produce hallucinated explanatory text.
- Detailed shim logs are debug-gated behind `ROSETTA_PDF2ZH_DEBUG=1`. When enabled, the shim logs `raw_user_preview`, `extracted_preview`, and `rwkv translation_preview` to both stderr and `pdf2zh-output/rosetta-pdf2zh-shim.log`. By default, these document-text previews are not written.
- Basic dogfood PDFs now complete end to end: import → source preview → pdf2zh/RWKV translation → translated PDF preview → exportable cached PDF. Remaining quality issues are mostly model/prompt/layout tradeoffs rather than broken plumbing.

## Next Steps

- Build the real macOS arm64 pdf2zh pack and pin its official download URL / SHA256 / size in the profile.
- Add the frontend settings/onboarding path that calls the pdf2zh installer when a PDF translation is requested and no managed pack is installed.
- Lock or patch the pdf2zh environment used by the pack.
- Run a larger dogfood set: academic paper, simple office PDF, table-heavy PDF, scanned/image-only PDF.
- Decide whether to keep using `pdf2zh 1.7.9` temporarily or move to the newer PDFMathTranslate/BabelDOC CLI expected by the original plan.

## Validation

- `cd rosetta-app && pnpm typecheck`
- `cd rosetta-app/src-tauri && cargo check`
- `cd rosetta-app/src-tauri && cargo test rosetta_jobs`

`cargo check` and `cargo test rosetta_jobs` still report the pre-existing dead-code warning for `PdfError::Encrypted`.
