# Linux Managed llama.cpp Vulkan Runtime

Date: 2026-07-13

## Summary

Added the managed translation runtime needed to exercise the complete Linux
PDF translation workflow.

## Change

- Added an enabled Linux x64 llama.cpp Vulkan runtime profile.
- Reused the existing `llama-cpp-chat-completions` provider and 0.4B Q8 GGUF.
- Added a checksum-pinned local staging script for the official llama.cpp
  `b9775` Ubuntu Vulkan archive and the translation model.
- Kept online runtime-pack installation out of scope until Linux release
  packaging is implemented.

No persistent data format changed.

## Validation

Run on Ubuntu Linux x64:

```bash
cd rosetta-app
HTTPS_PROXY=http://127.0.0.1:7890 \
  LLAMACPP_ARCHIVE_FILE=/tmp/llama-b9775-bin-ubuntu-vulkan-x64.tar.gz \
  RWKV_GGUF_FILE=/tmp/RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607-Q8_0.gguf \
  bash src-tauri/scripts/stage-llamacpp-linux-local.sh
pnpm typecheck
cd src-tauri
cargo check
cargo test managed_rwkv
cargo test rosetta_jobs
```

Results:

- llama.cpp `b9775` archive and GGUF size/SHA-256 verification passed.
- `llama-server --list-devices` detected four RTX 4090 Vulkan devices.
- The managed App lifecycle launched the Linux profile and passed its
  `/v1/models` readiness probe.
- A real `/completion` request translated `Translation runs locally.` to
  Chinese successfully.
- `pnpm typecheck`: passed.
- `cargo check`: passed with 9 existing Linux dead-code warnings.
- `cargo test managed_rwkv`: 51 passed, 0 failed.
- `cargo test rosetta_jobs`: 70 passed, 0 failed.
