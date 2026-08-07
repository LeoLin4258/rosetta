use serde::Serialize;

pub const PYMUPDF4LLM_VERSION: &str = "1.28.0";
pub const PYMUPDF_LAYOUT_VERSION: &str = "1.28.0";
pub const PYMUPDF_VERSION: &str = "1.28.0";
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy)]
pub struct PdfMarkdownProfile {
    pub id: &'static str,
    pub platform_os: &'static str,
    pub platform_arch: &'static str,
    pub directory_name: &'static str,
    pub archive_filename: &'static str,
    pub archive_bytes: u64,
    pub unpacked_bytes: u64,
    pub file_count: u64,
    pub archive_sha256: &'static str,
    pub download_urls: &'static [&'static str],
}

pub const WINDOWS_X64: PdfMarkdownProfile = PdfMarkdownProfile {
    id: "windows-x64-pdf-markdown",
    platform_os: "windows",
    platform_arch: "x86_64",
    directory_name: "windows-x64",
    archive_filename: "rosetta-pdf-markdown-windows-x64.zip",
    archive_bytes: 29_985_992,
    unpacked_bytes: 58_207_068,
    file_count: 101,
    archive_sha256: "f2e01a2df1a4c5aaa74114dbb49f1473b2082104f1aee23eeb3407ded13ac2fc",
    download_urls: &[
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-markdown-overlay-v2026.08.06.1/rosetta-pdf-markdown-windows-x64.zip",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-markdown-overlay-v2026.08.06.1/rosetta-pdf-markdown-windows-x64.zip",
    ],
};

pub const MACOS_ARM64: PdfMarkdownProfile = PdfMarkdownProfile {
    id: "macos-arm64-pdf-markdown",
    platform_os: "macos",
    platform_arch: "aarch64",
    directory_name: "macos-arm64",
    archive_filename: "rosetta-pdf-markdown-macos-arm64.tar.gz",
    archive_bytes: 34_622_507,
    unpacked_bytes: 67_564_550,
    file_count: 102,
    archive_sha256: "9a362d58227f6cb1159b8fa1520c23cc3ead951ae4d5f9abcf5153d9171fb6a9",
    download_urls: &[
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-markdown-overlay-v2026.08.06.1/rosetta-pdf-markdown-macos-arm64.tar.gz",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-markdown-overlay-v2026.08.06.1/rosetta-pdf-markdown-macos-arm64.tar.gz",
    ],
};

pub const LINUX_X64: PdfMarkdownProfile = PdfMarkdownProfile {
    id: "linux-x64-pdf-markdown",
    platform_os: "linux",
    platform_arch: "x86_64",
    directory_name: "linux-x64",
    archive_filename: "rosetta-pdf-markdown-linux-x64.tar.gz",
    archive_bytes: 36_480_503,
    unpacked_bytes: 73_638_161,
    file_count: 102,
    archive_sha256: "fa2ca9e5e66e2f1930cbb200a2b4a9001d5e8f4e2c256d45844462e0cdab447e",
    download_urls: &[
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-markdown-overlay-v2026.08.06.1/rosetta-pdf-markdown-linux-x64.tar.gz",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-markdown-overlay-v2026.08.06.1/rosetta-pdf-markdown-linux-x64.tar.gz",
    ],
};

const PROFILES: &[PdfMarkdownProfile] = &[WINDOWS_X64, MACOS_ARM64, LINUX_X64];

pub fn current_profile() -> Option<&'static PdfMarkdownProfile> {
    PROFILES.iter().find(|profile| {
        profile.platform_os == std::env::consts::OS
            && profile.platform_arch == std::env::consts::ARCH
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfMarkdownProfileSummary {
    pub id: &'static str,
    pub platform_os: &'static str,
    pub platform_arch: &'static str,
    pub archive_bytes: u64,
    pub unpacked_bytes: u64,
    pub file_count: u64,
}

impl From<&'static PdfMarkdownProfile> for PdfMarkdownProfileSummary {
    fn from(profile: &'static PdfMarkdownProfile) -> Self {
        Self {
            id: profile.id,
            platform_os: profile.platform_os,
            platform_arch: profile.platform_arch,
            archive_bytes: profile.archive_bytes,
            unpacked_bytes: profile.unpacked_bytes,
            file_count: profile.file_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_pin_exact_release_artifacts() {
        for profile in PROFILES {
            assert!(profile.archive_bytes > 0);
            assert!(profile.unpacked_bytes > profile.archive_bytes);
            assert!(profile.file_count > 0);
            assert_eq!(profile.archive_sha256.len(), 64);
            assert_eq!(profile.download_urls.len(), 2);
        }
        assert_eq!(WINDOWS_X64.archive_bytes + 366_073_383, 396_059_375);
    }
}
