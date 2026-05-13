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
    /// Whether this profile is part of the v1 surface. Windows profile stays
    /// `false` until Phase 8; Phase 3 reads this to skip Windows in dispatch.
    pub enabled: bool,
    /// Backend flag passed to `rwkv_server --backend`.
    pub backend: &'static str,
    /// Sidecar filename as it appears in the Tauri bundle (Tauri externalBin
    /// convention: `<name>-<target-triple>`). On macOS the binary ends up in
    /// `<App>.app/Contents/MacOS/<filename>`.
    pub sidecar_binary_name: &'static str,
    /// Tokenizer file shipped as a Tauri bundle resource alongside the app.
    pub tokenizer_filename: &'static str,
    /// Subdirectory under `<app-local-data>/models/` where the model file lives.
    pub model_directory_name: &'static str,
    /// Model filename inside that directory.
    pub model_filename: &'static str,
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

/// macOS Apple Silicon profile — the v1 default per ADR 0003.
pub const MACOS_ARM64_WEBRWKV: RuntimeProfile = RuntimeProfile {
    id: "macos-arm64-webrwkv",
    provider_id: "rwkv-mobile-batch-chat",
    platform_os: "macos",
    platform_arch: "aarch64",
    enabled: true,
    backend: "web-rwkv",
    sidecar_binary_name: "rwkv-server-aarch64-apple-darwin",
    tokenizer_filename: "b_rwkv_vocab_v20230424.txt",
    model_directory_name: "rwkv-translate-1.5b-nf4",
    model_filename: "RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab",
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

/// Windows libtorch profile — placeholder for Phase 8.
///
/// Kept as a const so the profile abstraction is exercised by more than one
/// instance even before the Windows lifecycle code lands. `enabled: false`
/// means the status command reports `unsupported` even on Windows hosts
/// until the supporting code lands.
#[allow(dead_code)]
pub const WINDOWS_AMD64_LIBTORCH: RuntimeProfile = RuntimeProfile {
    id: "windows-amd64-libtorch",
    provider_id: "rwkv-lightning-contents",
    platform_os: "windows",
    platform_arch: "x86_64",
    enabled: false,
    backend: "libtorch",
    sidecar_binary_name: "rwkv_lightning.exe",
    tokenizer_filename: "rwkv_vocab_v20230424.txt",
    model_directory_name: "rwkv-v7-g1-translate-1.5b",
    model_filename: "RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth",
    model_size_bytes: 3_055_445_546,
    model_sha256: "b51051a35949cbd6189da3d99b2bd9ae632d5665716a8e647abbe208f21120fa",
    model_download_urls: &[
        "https://modelscope.cn/models/AlicLi/RWKV_v7_G1_Translate/resolve/master/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118.pth",
    ],
    supported_directions: &["en-zh", "zh-en"],
    model_name_arg: "rwkv-translate",
    health_path: "/health",
    batch_chat_path: "/v1/chat/completions",
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
    ALL_PROFILES
        .iter()
        .find(|profile| profile.enabled && profile.platform_os == os && profile.platform_arch == arch)
}

/// Profile-summary shape exposed to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProfileSummary {
    pub id: &'static str,
    pub provider_id: &'static str,
    pub platform_os: &'static str,
    pub platform_arch: &'static str,
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
            backend: profile.backend,
            model_filename: profile.model_filename,
            model_size_bytes: profile.model_size_bytes,
            model_sha256: profile.model_sha256,
            supported_directions: profile.supported_directions,
            bind_host: profile.bind_host,
        }
    }
}

const ALL_PROFILES: &[RuntimeProfile] = &[MACOS_ARM64_WEBRWKV, WINDOWS_AMD64_LIBTORCH];

#[cfg(test)]
#[allow(clippy::assertions_on_constants)] // intentional regression guards on const values
mod tests {
    use super::*;

    #[test]
    fn macos_profile_is_enabled_and_targets_apple_silicon() {
        assert!(MACOS_ARM64_WEBRWKV.enabled);
        assert_eq!(MACOS_ARM64_WEBRWKV.platform_os, "macos");
        assert_eq!(MACOS_ARM64_WEBRWKV.platform_arch, "aarch64");
        assert_eq!(MACOS_ARM64_WEBRWKV.provider_id, "rwkv-mobile-batch-chat");
        assert_eq!(MACOS_ARM64_WEBRWKV.bind_host, "127.0.0.1");
        assert_eq!(MACOS_ARM64_WEBRWKV.backend, "web-rwkv");
    }

    #[test]
    fn windows_profile_is_disabled_until_phase_8() {
        assert!(!WINDOWS_AMD64_LIBTORCH.enabled);
    }

    #[test]
    fn current_profile_returns_some_on_supported_arches_only() {
        let resolved = current_profile();
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("macos", "aarch64") => assert!(
                resolved.is_some_and(|p| p.id == "macos-arm64-webrwkv"),
                "expected macOS arm64 profile"
            ),
            // Windows profile is currently disabled — even on win/x86_64 the
            // resolved profile should be None until Phase 8 flips `enabled`.
            _ => assert!(resolved.is_none(), "expected no profile on unsupported host"),
        }
    }
}
