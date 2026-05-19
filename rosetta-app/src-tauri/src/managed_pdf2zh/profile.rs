use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub struct Pdf2zhProfile {
    pub id: &'static str,
    pub platform_os: &'static str,
    pub platform_arch: &'static str,
    pub enabled: bool,
    pub pack_directory_name: &'static str,
    pub bin_relative_path: &'static str,
}

pub const MACOS_ARM64_PDF2ZH: Pdf2zhProfile = Pdf2zhProfile {
    id: "macos-arm64-pdf2zh",
    platform_os: "macos",
    platform_arch: "aarch64",
    enabled: true,
    pack_directory_name: "macos-arm64",
    bin_relative_path: "bin/pdf2zh",
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
