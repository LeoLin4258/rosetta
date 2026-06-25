# 2026-06-24 Windows llama.cpp Vulkan runtime

## Initial implementation (Codex)

- Added ADR 0007 and the Windows llama.cpp Vulkan implementation plan.
- Added the Windows x64 llama.cpp Vulkan managed runtime profile as the
  recommended Windows profile.
- Kept RWKV Lightning CUDA as a secondary NVIDIA profile with a development
  warning.
- Added the `llama-cpp-chat-completions` provider for local translation.
- Added llama.cpp support to the PDF OpenAI shim.
- Added automatic download source probing so Rosetta silently chooses a fast
  available source instead of asking users to pick a mirror.
- Added post-install Vulkan validation through `llama-server.exe --list-devices`.

## Smoke-test fixes

The Codex-authored implementation was tested on an AMD Ryzen 7 8745HS +
Radeon 780M (Vulkan 1.3.302, 16 GB shared VRAM, driver 24.30.18) running
Windows 11. Three blocking issues were found and fixed in order.

### 1. SHA256 mismatch after model download

**Symptom**: Onboarding failed at SHA256 verification immediately after
downloading the GGUF model.

**Root cause**: The SHA256 in `profile.rs` did not match the actual file on
the mirror. The hash was likely computed against a different build of the
model file.

**Fix**: Updated `WINDOWS_AMD64_LLAMACPP_VULKAN.model_sha256` to
`f0f1c64455d075236df309457e4730fe763489e5fc8c038ce3f29d9963dec96b`, verified
against the canonical ModelScope artifact.

### 2. Vulkan `ErrorExtensionNotPresent` crash in CPU fallback

**Symptom**: On GPUs missing the `VK_KHR_cooperative_matrix2` Vulkan
extension (including the test Radeon 780M), `llama-server.exe` crashed during
startup even with `--gpu-layers 0`:

```
ggml_vk_instance_init: failed to initialize Vulkan instance: ErrorExtensionNotPresent
vkCreateFence: Invalid device [-85858585]
```

**Root cause**: `--gpu-layers 0` prevents placing model layers on the GPU but
does *not* prevent Vulkan backend initialization. The backend still tries to
create a VkDevice and enumerate compute extensions, crashing when a required
extension is absent.

**Fix**: Added `--device none` alongside `--gpu-layers 0` in the CPU-fallback
branch of `lifecycle.rs::build_command_args`. `--device none` fully disables
Vulkan backend initialization so no VkDevice is ever created.

```rust
if gpu_layers_override.is_some() {
    args.extend(["--device".to_string(), "none".to_string()]);
}
```

Tests updated: `llama_cpp_cpu_fallback_uses_gpu_layers_zero_and_device_none`
asserts both flags; the normal-path test asserts `--device` is absent.

### 3. Translation fails: wrong prompt format for RWKV translate model

**Symptom**: Onboarding completed and the local runtime started on port
56845, but translating any Markdown document immediately failed with
"翻译失败，请检查 API 配置或网络". The underlying HTTP request returned a 200
but the response content was empty or echoed the source text.

**Root cause**: The `llama-cpp-chat-completions` provider sent standard
OpenAI-style chat completions to `/v1/chat/completions`:

```json
{
  "messages": [
    {"role": "system", "content": "You are a translation engine. Translate..."},
    {"role": "user", "content": "Hello, world."}
  ]
}
```

The RWKV v7 G1d 0.4B Translate model does **not** understand this format.
It uses a role-based prompt where language names serve as role labels:

```
English: Hello, world.

Chinese:
```

This was confirmed by examining
[RWKV_APP](../../RWKV_APP/lib/store/translator.dart)'s `SetUserRole` /
`SetResponseRole` API and by direct `/completion` testing against the running
llama-server.

**Fix** (6 files):

- **`rwkv_providers/llama_cpp_chat.rs`**: Switched from `/v1/chat/completions`
  to `/completion`. Build prompt as `{SourceLang}: {text}\n\n{TargetLang}:`.
  Parse the flat `{content: "..."}` response instead of nested
  `{choices: [{message: {content}}]}`.
- **`rwkv_providers/mod.rs`**: Added `source_lang: &str` to
  `ProviderTranslateBatch` so the llama-cpp provider knows the source
  language for prompt construction.
- **`managed_rwkv/profile.rs`**: Updated `batch_chat_path` from
  `/v1/chat/completions` to `/completion`.
- **`rwkv_api.rs`**: Threaded `source_lang` through all
  `ProviderTranslateBatch` call sites and `translate_batch_via_llama_cpp`.
- **`managed_pdf2zh/openai_shim.rs`**: Threaded `source_lang` through
  `mobile_batch_processor` and `llama_cpp_batch_processor`.
- **`rwkv_providers/mobile_batch_chat.rs`**: Added `source_lang` to probe
  batch construction (field ignored by mobile provider but required by the
  shared struct).

### 4. Low default parallelism

**Symptom**: Translation throughput felt noticeably slow (~3 concurrent
segments).

**Root cause**: `DEFAULT_PARALLEL_REQUESTS` was set to 4, which also
controlled the llama-server `--parallel` slot count. The `plan_batches`
length-bucket policy further halved the ceiling for medium-length segments
(301–1200 chars → ceiling/2 = 2).

**Fix**: Raised `DEFAULT_PARALLEL_REQUESTS` from 4 to 16, matching the
macOS markdown batch ceiling. The 0.4B model is small enough to serve 16
concurrent completions even in CPU-only mode.

### 5. Windows llama.cpp runtime GitHub mirror

**Symptom**: Mainland China users can have slow or blocked access to the
llama.cpp Windows Vulkan runtime ZIP on GitHub Releases.

**Fix**: Added a githubdog mirror URL to
`WINDOWS_AMD64_LLAMACPP_VULKAN.runtime_download_urls`. The existing automatic
source probing still picks the fastest reachable source and falls back without
asking the user to choose a mirror.

## Test results

- 180 Rust unit tests pass (`cargo test`).
- TypeScript type check passes (`tsc --noEmit`).
- Manual smoke test on AMD Radeon 780M: onboarding completes, Markdown
  translation produces correct Chinese output, CPU-fallback mode is stable.
