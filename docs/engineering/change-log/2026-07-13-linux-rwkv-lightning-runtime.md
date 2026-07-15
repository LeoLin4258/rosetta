# Linux RWKV Lightning Runtime

Date: 2026-07-13

## Summary

Added RWKV Lightning CUDA as the preferred managed translation runtime on
supported NVIDIA Linux systems, with llama.cpp Vulkan retained as fallback.

## Change

- Added a checksum-pinned Linux x64 Lightning V1.0.3 profile and local staging
  script.
- Preferred the resumable HF mirror for the Linux PTH after ModelScope ended a
  real 900 MB transfer early; ModelScope remains a fallback.
- Extended SM75+ `nvidia-smi` detection to Linux.
- Made frontend runtime selection platform-neutral: supported Lightning first,
  then MLX or llama.cpp as applicable, while preserving a saved user choice.
- Added Linux `LD_LIBRARY_PATH` launch support and executable permission repair
  for ZIP-installed runtimes.
- Reused the existing `rwkv-lightning-contents` batch provider and 0.4B PTH.

No persistent data format changed.

## Validation

Run on Ubuntu 24.04 x64 with four RTX 4090 GPUs:

- Runtime ZIP size and SHA-256 passed (`430,509,983` bytes,
  `403c34dd...1005`); the executable reported no missing `ldd` dependencies.
- PTH size and SHA-256 passed (`901,775,740` bytes,
  `b9a1b013...0527`).
- `nvidia-smi` reported four SM89 GPUs.
- The server became ready on loopback in three seconds and `/v1/models`
  returned the expected translation model.
- A real 16-segment `/v1/batch/completions` request returned 16 Chinese
  translations in 0.531 seconds on the first request. Three warm requests took
  0.318, 0.302, and 0.274 seconds. The earlier llama.cpp Vulkan benchmark on
  the same host took about 2.315 seconds.
- `pnpm typecheck`: passed on Windows and Ubuntu.
- `cargo check`: passed on Windows and Ubuntu. Ubuntu retained nine existing
  platform dead-code warnings.
- `cargo test managed_rwkv`: 52 passed on Windows and Ubuntu.
- `cargo test rosetta_jobs`: 70 passed on Ubuntu.
- `cargo fmt --check`: passed on Windows. The intentionally minimal Ubuntu
  toolchain does not include the `rustfmt` component, so it was not installed.
