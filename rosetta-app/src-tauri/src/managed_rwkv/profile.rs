//! Runtime profile metadata.
//!
//! A `RuntimeProfile` captures everything that's different across platforms
//! and backends: which sidecar binary to spawn, which model file to load,
//! which translation provider to dispatch through, what the health endpoint
//! looks like, and so on. The Phase 3 macOS implementation references one
//! profile at a time; Phase 8 will add Windows as a second profile and the
//! status/lifecycle code already takes the profile as a parameter so that
//! switch is cheap.

use serde::Serialize;

/// Static description of a managed sidecar runtime + its companion model.
///
/// All fields are `&'static` so a profile is a `const` and can be matched
/// in `cfg!`-gated dispatch without allocation.
///
/// `batch_chat_path` is currently unused at the Phase 3 boundary because the
/// translation runner reaches it through the `rwkv-mobile-batch-chat`
/// provider in `rwkv_providers`, not through the runtime module. It's kept
/// in the profile so Phase 6's "runtime → provider config" wiring has one
/// source of truth for the URL shape.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub struct RuntimeProfile {
    /// Stable identifier used in logs, manifests, and the UI (`macos-arm64-webrwkv`).
    pub id: &'static str,
    /// Provider id the translation runner should dispatch to when this
    /// runtime is ready. Mirrors `RwkvProviderId` on the TypeScript side.
    pub provider_id: &'static str,
    /// `std::env::consts::OS` value the profile is valid on.
    pub platform_os: &'static str,
    /// `std::env::consts::ARCH` value the profile is valid on.
    pub platform_arch: &'static str,
    /// Human-readable runtime label for diagnostics and Settings.
    pub runtime_label: &'static str,
    /// Short hardware/runtime requirement shown to users. This must be
    /// explicit for CUDA packs so AMD / Intel machines do not look supported.
    pub hardware_requirement: &'static str,
    /// Whether this profile is part of the v1 surface. Windows profile stays
    /// `false` until Phase 8; Phase 3 reads this to skip Windows in dispatch.
    pub enabled: bool,
    /// Backend flag passed to `rwkv_server --backend`.
    pub backend: &'static str,
    /// Sidecar filename as it appears in the Tauri bundle (Tauri externalBin
    /// convention: `<name>-<target-triple>`). On macOS the binary ends up in
    /// `<App>.app/Contents/MacOS/<filename>`.
    pub sidecar_binary_name: &'static str,
    /// Optional app-data managed runtime directory. macOS ships the sidecar
    /// inside the app bundle; Windows CUDA dogfood installs a whole runtime
    /// pack under `<app-local-data>/managed-rwkv/runtimes/<name>/`.
    pub managed_runtime_directory_name: Option<&'static str>,
    /// Archive filename for app-data managed runtime packs.
    pub runtime_archive_filename: Option<&'static str>,
    /// Exact runtime archive size in bytes, if this profile installs one.
    pub runtime_archive_size_bytes: Option<u64>,
    /// SHA256 of the runtime archive, if this profile installs one.
    pub runtime_archive_sha256: Option<&'static str>,
    /// Runtime pack URLs to try after local/env sources. Dogfood may leave
    /// this empty while using a locally downloaded artifact.
    pub runtime_download_urls: &'static [&'static str],
    /// Directory inside the runtime pack that must be prepended to PATH before
    /// spawning the backend. Windows CUDA uses `lib`.
    pub runtime_library_dir_name: Option<&'static str>,
    /// Tokenizer file shipped as a Tauri bundle resource alongside the app.
    pub tokenizer_filename: &'static str,
    /// Subdirectory under `<app-local-data>/models/` where the model file lives.
    pub model_directory_name: &'static str,
    /// Model filename (or zip archive name) inside that directory.
    pub model_filename: &'static str,
    /// When `true`, `model_filename` is a zip archive; after download and
    /// SHA256 verification it is extracted in-place, then the zip is deleted.
    /// `layout.model_file` points to the zip during download and to the
    /// extracted directory (same stem, no extension) once installed.
    pub model_is_zip: bool,
    /// Exact size in bytes of the model file the SHA256 was computed against.
    /// HEAD requests verify this matches before download starts; mismatched
    /// content-length fails fast with `artifact-corrupted` instead of wasting
    /// bandwidth.
    pub model_size_bytes: u64,
    /// Hex SHA256 of the canonical model file. Phase 0 verification on
    /// 2026-05-13 used this exact file.
    pub model_sha256: &'static str,
    /// Mirror URLs to try in order. Phase 4 walks them top-to-bottom and
    /// stops at the first one that returns a 2xx response (HEAD + GET).
    /// ModelScope is intentionally not present for the WebRWKV `.prefab`
    /// path — that file does not currently exist on AlicLi's ModelScope
    /// repo, and the model author's parallel ModelScope namespace returned
    /// empty file lists when probed on 2026-05-13.
    pub model_download_urls: &'static [&'static str],
    /// Languages the model is trained for, as ISO direction codes.
    pub supported_directions: &'static [&'static str],
    /// `--model-name` argument passed to `rwkv_server` so the model name in
    /// its JSON responses matches what the adapter expects.
    pub model_name_arg: &'static str,
    /// Health probe HTTP path.
    pub health_path: &'static str,
    /// Batch chat endpoint path used by the `rwkv-mobile-batch-chat` adapter.
    pub batch_chat_path: &'static str,
    /// Loopback host the sidecar must bind to. Never overridden at runtime.
    pub bind_host: &'static str,
}

/// macOS Apple Silicon profile — MLX backend, 0.4B model (switched 2026-06-10).
pub const MACOS_ARM64_MLX: RuntimeProfile = RuntimeProfile {
    id: "macos-arm64-mlx",
    provider_id: "rwkv-mobile-batch-chat",
    platform_os: "macos",
    platform_arch: "aarch64",
    runtime_label: "RWKV Mobile MLX",
    hardware_requirement: "Apple Silicon",
    enabled: true,
    backend: "mlx",
    sidecar_binary_name: "rwkv-server-aarch64-apple-darwin",
    managed_runtime_directory_name: None,
    runtime_archive_filename: None,
    runtime_archive_size_bytes: None,
    runtime_archive_sha256: None,
    runtime_download_urls: &[],
    runtime_library_dir_name: None,
    tokenizer_filename: "b_rwkv_vocab_v20230424.txt",
    model_directory_name: "rwkv7-0.4b-mlx-6bit",
    model_filename: "rwkv7-0.4B-g1d-translate-20260607-ctx4096-mlx-6bit.zip",
    model_is_zip: true,
    model_size_bytes: 377_343_557,
    model_sha256: "ae1109105ce91627406972c25d618da2922f74331f773b18975c7e4e290bc226",
    model_download_urls: &[
        "https://huggingface.co/mollysama/rwkv-mobile-models/resolve/main/mlx/rwkv7-0.4B-g1d-translate-20260607-ctx4096-mlx-6bit.zip",
        "https://hf-mirror.com/mollysama/rwkv-mobile-models/resolve/main/mlx/rwkv7-0.4B-g1d-translate-20260607-ctx4096-mlx-6bit.zip",
    ],
    supported_directions: &["en-zh", "zh-en"],
    model_name_arg: "rwkv-translate",
    health_path: "/health",
    batch_chat_path: "/v1/batch/chat",
    bind_host: "127.0.0.1",
};

/// macOS Apple Silicon profile — WebRWKV backend (disabled, superseded by MLX).
pub const MACOS_ARM64_WEBRWKV: RuntimeProfile = RuntimeProfile {
    id: "macos-arm64-webrwkv",
    provider_id: "rwkv-mobile-batch-chat",
    platform_os: "macos",
    platform_arch: "aarch64",
    runtime_label: "RWKV Mobile WebRWKV",
    hardware_requirement: "Apple Silicon",
    enabled: false,
    backend: "web-rwkv",
    sidecar_binary_name: "rwkv-server-aarch64-apple-darwin",
    managed_runtime_directory_name: None,
    runtime_archive_filename: None,
    runtime_archive_size_bytes: None,
    runtime_archive_sha256: None,
    runtime_download_urls: &[],
    runtime_library_dir_name: None,
    tokenizer_filename: "b_rwkv_vocab_v20230424.txt",
    model_directory_name: "rwkv-translate-1.5b-nf4",
    model_filename: "RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab",
    model_is_zip: false,
    // Exact byte count of the file Phase 0 validated against on 2026-05-13.
    // Matches the Content-Length the HuggingFace CDN reports.
    model_size_bytes: 1_355_373_863,
    model_sha256: "f6eeb1fff051bcba88539f641993d9a45e4f697f2db37e3bf0fcdd09bff2ef15",
    // HuggingFace direct first; reqwest honors HTTPS_PROXY so users behind
    // Clash / corporate proxy can still reach it. hf-mirror.com was unreliable
    // on 2026-05-13 (returned LFS pointer files / empty bodies) but is kept
    // as a fallback because it sometimes works without a proxy.
    model_download_urls: &[
        "https://huggingface.co/mollysama/rwkv-mobile-models/resolve/main/WebRWKV/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab",
        "https://hf-mirror.com/mollysama/rwkv-mobile-models/resolve/main/WebRWKV/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab",
    ],
    supported_directions: &["en-zh", "zh-en"],
    model_name_arg: "rwkv-translate",
    health_path: "/health",
    batch_chat_path: "/v1/batch/chat",
    bind_host: "127.0.0.1",
};

/// Windows NVIDIA CUDA profile — Alic-Li rwkv_lightning_cuda V1.0.0 dogfood.
///
/// This intentionally replaces the old Windows libtorch placeholder. The
/// runtime pack is managed in app data instead of bundled as a Tauri sidecar.
#[allow(dead_code)]
pub const WINDOWS_AMD64_CUDA: RuntimeProfile = RuntimeProfile {
    id: "windows-amd64-rwkv-lightning-cuda",
    provider_id: "rwkv-lightning-contents",
    platform_os: "windows",
    platform_arch: "x86_64",
    runtime_label: "RWKV Lightning NVIDIA CUDA",
    hardware_requirement: "NVIDIA GPU with CUDA, compute capability sm75 or newer",
    enabled: true,
    backend: "cuda-openai",
    sidecar_binary_name: "rwkv_lighting_cuda.exe",
    managed_runtime_directory_name: Some("rwkv-lightning-cuda-sm75-msvc"),
    runtime_archive_filename: Some("RWKV_lightning_CUDA_sm75+_Win_MSVC.7z"),
    runtime_archive_size_bytes: Some(437_910_029),
    runtime_archive_sha256: Some(
        "454a41ad215d4adf156c261991f0732ed22e64e7eed9780321848050435d7a7c",
    ),
    runtime_download_urls: &[
        "https://github.com/Alic-Li/rwkv_lightning_cuda/releases/download/V1.0.0/RWKV_lightning_CUDA_sm75+_Win_MSVC.7z",
        "https://githubdog.com/https://github.com/Alic-Li/rwkv_lightning_cuda/releases/download/V1.0.0/RWKV_lightning_CUDA_sm75+_Win_MSVC.7z",
    ],
    runtime_library_dir_name: Some("lib"),
    tokenizer_filename: "rwkv_vocab_v20230424.txt",
    model_directory_name: "rwkv7-0.4b-translate-windows-pth",
    model_filename: "RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607.pth",
    model_is_zip: false,
    model_size_bytes: 901_775_740,
    model_sha256: "b9a1b013c3a938515f8b9bc23c28d815fa6f839eef77a943e92e7e70d35a0527",
    model_download_urls: &[
        "https://huggingface.co/Alic-Li/RWKV_v7_G1_Translate/resolve/main/RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607.pth",
        "https://hf-mirror.com/Alic-Li/RWKV_v7_G1_Translate/resolve/main/RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607.pth",
        "https://modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/resolve/master/RWKV_v7_G1d_0.4B_Translate_ctx4096_20260607.pth",
    ],
    supported_directions: &["en-zh", "zh-en"],
    model_name_arg: "rwkv-translate",
    health_path: "/v1/models",
    batch_chat_path: "/v1/batch/completions",
    bind_host: "127.0.0.1",
};

/// Returns the profile valid for the current host, or `None` when the
/// platform is unsupported (e.g. Intel Mac or Linux).
pub fn current_profile() -> Option<&'static RuntimeProfile> {
    // Match against the *runtime* OS+ARCH so a single Rosetta build can
    // honestly report "unsupported" on Intel Macs / non-macOS hosts instead
    // of `cfg!(target_os)` baking the answer at compile time.
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    ALL_PROFILES.iter().find(|profile| {
        profile.enabled && profile.platform_os == os && profile.platform_arch == arch
    })
}

/// Profile-summary shape exposed to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileSummary {
    pub id: &'static str,
    pub provider_id: &'static str,
    pub platform_os: &'static str,
    pub platform_arch: &'static str,
    pub runtime_label: &'static str,
    pub hardware_requirement: &'static str,
    pub backend: &'static str,
    pub model_filename: &'static str,
    pub model_size_bytes: u64,
    pub model_sha256: &'static str,
    pub supported_directions: &'static [&'static str],
    pub bind_host: &'static str,
}

impl RuntimeProfileSummary {
    pub fn from_profile(profile: &'static RuntimeProfile) -> Self {
        Self {
            id: profile.id,
            provider_id: profile.provider_id,
            platform_os: profile.platform_os,
            platform_arch: profile.platform_arch,
            runtime_label: profile.runtime_label,
            hardware_requirement: profile.hardware_requirement,
            backend: profile.backend,
            model_filename: profile.model_filename,
            model_size_bytes: profile.model_size_bytes,
            model_sha256: profile.model_sha256,
            supported_directions: profile.supported_directions,
            bind_host: profile.bind_host,
        }
    }
}

const ALL_PROFILES: &[RuntimeProfile] = &[MACOS_ARM64_MLX, MACOS_ARM64_WEBRWKV, WINDOWS_AMD64_CUDA];

#[cfg(test)]
#[allow(clippy::assertions_on_constants)] // intentional regression guards on const values
mod tests {
    use super::*;

    #[test]
    fn macos_profile_is_enabled_and_targets_apple_silicon() {
        assert!(MACOS_ARM64_MLX.enabled);
        assert_eq!(MACOS_ARM64_MLX.platform_os, "macos");
        assert_eq!(MACOS_ARM64_MLX.platform_arch, "aarch64");
        assert_eq!(MACOS_ARM64_MLX.provider_id, "rwkv-mobile-batch-chat");
        assert_eq!(MACOS_ARM64_MLX.hardware_requirement, "Apple Silicon");
        assert_eq!(MACOS_ARM64_MLX.bind_host, "127.0.0.1");
        assert_eq!(MACOS_ARM64_MLX.backend, "mlx");
    }

    #[test]
    fn webrwkv_profile_is_disabled() {
        assert!(!MACOS_ARM64_WEBRWKV.enabled);
    }

    #[test]
    fn windows_profile_is_enabled_for_cuda_dogfood() {
        assert!(WINDOWS_AMD64_CUDA.enabled);
        assert_eq!(WINDOWS_AMD64_CUDA.platform_os, "windows");
        assert_eq!(WINDOWS_AMD64_CUDA.backend, "cuda-openai");
        assert!(WINDOWS_AMD64_CUDA.hardware_requirement.contains("NVIDIA"));
        assert!(WINDOWS_AMD64_CUDA.hardware_requirement.contains("sm75"));
        assert_eq!(WINDOWS_AMD64_CUDA.health_path, "/v1/models");
        assert!(WINDOWS_AMD64_CUDA.managed_runtime_directory_name.is_some());
    }

    #[test]
    fn current_profile_returns_some_on_supported_arches_only() {
        let resolved = current_profile();
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => {
                assert!(resolved.is_some_and(|p| p.id == "macos-arm64-mlx"));
            }
            ("windows", "x86_64") => {
                assert!(resolved.is_some_and(|p| p.id == "windows-amd64-rwkv-lightning-cuda"));
            }
            _ => assert!(
                resolved.is_none(),
                "expected no profile on unsupported host"
            ),
        }
    }
}
