use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct Pdf2zhProfile {
    pub id: &'static str,
    pub platform_os: &'static str,
    pub platform_arch: &'static str,
    pub enabled: bool,
    pub pack_directory_name: &'static str,
    pub bin_relative_path: &'static str,
    pub pack_filename: &'static str,
    pub pack_size_bytes: Option<u64>,
    pub pack_sha256: Option<&'static str>,
    pub pack_download_urls: &'static [&'static str],
}

// pack_size_bytes, pack_sha256, and pack_download_urls are filled in after
// running build-pdf2zh-pack-macos-arm64.sh and uploading the artifact to a
// GitHub Release under LeoLin4258/rosetta-assets with tag
// pdf-layout-pack-macos-arm64-vYYYY.MM.DD.N.
//
// pack_download_urls may list a primary URL followed by mirror URLs; the
// installer tries them in order and falls back automatically.
pub const MACOS_ARM64_PDF2ZH: Pdf2zhProfile = Pdf2zhProfile {
    id: "macos-arm64-pdf2zh",
    platform_os: "macos",
    platform_arch: "aarch64",
    enabled: true,
    pack_directory_name: "macos-arm64",
    bin_relative_path: "bin/pdf2zh",
    pack_filename: "rosetta-pdf2zh-macos-arm64.tar.gz",
    pack_size_bytes: Some(257_745_643),
    pack_sha256: Some("06de231bfc3e3bceb31ecb300772e2ba6b99569d119d3740de67849f37345fcd"),
    pack_download_urls: &[
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-macos-arm64-v2026.05.20.1/rosetta-pdf2zh-macos-arm64.tar.gz",
    ],
};

const ALL_PROFILES: &[Pdf2zhProfile] = &[MACOS_ARM64_PDF2ZH];

pub fn current_profile() -> Option<&'static Pdf2zhProfile> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    ALL_PROFILES
        .iter()
        .find(|profile| profile.enabled && profile.platform_os == os && profile.platform_arch == arch)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pdf2zhProfileSummary {
    pub id: &'static str,
    pub platform_os: &'static str,
    pub platform_arch: &'static str,
    pub pack_directory_name: &'static str,
}

impl Pdf2zhProfileSummary {
    pub fn from_profile(profile: &'static Pdf2zhProfile) -> Self {
        Self {
            id: profile.id,
            platform_os: profile.platform_os,
            platform_arch: profile.platform_arch,
            pack_directory_name: profile.pack_directory_name,
        }
    }
}
