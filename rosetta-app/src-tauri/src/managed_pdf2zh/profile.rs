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
// running the matching build-pdf2zh-pack-* script and uploading the artifact
// to a GitHub Release under LeoLin4258/rosetta-assets with a platform-specific
// pdf-layout-pack-* tag.
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
    pack_size_bytes: Some(406_417_600),
    pack_sha256: Some("6a43e390af9cc5c4518af960696e3bb6322c247177d619585edb719897090635"),
    pack_download_urls: &[
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-macos-arm64-v2026.07.10.1/rosetta-pdf2zh-macos-arm64.tar.gz",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-macos-arm64-v2026.07.10.1/rosetta-pdf2zh-macos-arm64.tar.gz",
    ],
};

pub const WINDOWS_AMD64_PDF2ZH: Pdf2zhProfile = Pdf2zhProfile {
    id: "windows-amd64-pdf2zh",
    platform_os: "windows",
    platform_arch: "x86_64",
    enabled: true,
    pack_directory_name: "windows-amd64",
    bin_relative_path: "python/python.exe",
    pack_filename: "rosetta-pdf2zh-windows-amd64.zip",
    pack_size_bytes: Some(349_529_488),
    pack_sha256: Some("80680b6fd94fba53a256e323337790bfd997af03c4703db0f99680a9dc1b2246"),
    pack_download_urls: &[
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.07.10.1/rosetta-pdf2zh-windows-amd64.zip",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.07.10.1/rosetta-pdf2zh-windows-amd64.zip",
    ],
};

/// Linux x64 release profile built from the pinned Rosetta PDFMathTranslate
/// fork and a relocatable python-build-standalone runtime.
pub const LINUX_X64_PDF2ZH: Pdf2zhProfile = Pdf2zhProfile {
    id: "linux-x64-pdf2zh",
    platform_os: "linux",
    platform_arch: "x86_64",
    enabled: true,
    pack_directory_name: "linux-x64",
    bin_relative_path: "bin/pdf2zh",
    pack_filename: "rosetta-pdf2zh-linux-x64.tar.gz",
    pack_size_bytes: Some(510_384_173),
    pack_sha256: Some("4f71a0ea881f899d2c10a8a76874f453b4829840f8a1f36efcc19fde9bfd3f5d"),
    pack_download_urls: &[
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-linux-x64-v2026.07.14.1/rosetta-pdf2zh-linux-x64.tar.gz",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-linux-x64-v2026.07.14.1/rosetta-pdf2zh-linux-x64.tar.gz",
    ],
};

const ALL_PROFILES: &[Pdf2zhProfile] =
    &[MACOS_ARM64_PDF2ZH, WINDOWS_AMD64_PDF2ZH, LINUX_X64_PDF2ZH];

pub fn current_profile() -> Option<&'static Pdf2zhProfile> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    ALL_PROFILES.iter().find(|profile| {
        profile.enabled && profile.platform_os == os && profile.platform_arch == arch
    })
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

#[cfg(test)]
mod tests {
    use super::{current_profile, LINUX_X64_PDF2ZH, MACOS_ARM64_PDF2ZH, WINDOWS_AMD64_PDF2ZH};

    #[test]
    fn macos_pdf_pack_defaults_to_mainland_download_mirror() {
        assert!(
            MACOS_ARM64_PDF2ZH
                .pack_download_urls
                .first()
                .is_some_and(|url| url.starts_with("https://githubdog.com/https://github.com/")),
            "githubdog mirror should be the default for mainland users"
        );
        assert!(
            MACOS_ARM64_PDF2ZH
                .pack_download_urls
                .iter()
                .any(|url| url.starts_with("https://github.com/")),
            "GitHub release URL should remain available as fallback"
        );
    }

    #[test]
    fn windows_pdf_pack_defaults_to_mainland_download_mirror() {
        assert!(WINDOWS_AMD64_PDF2ZH
            .pack_download_urls
            .first()
            .is_some_and(|url| url.starts_with("https://githubdog.com/")));
        assert!(WINDOWS_AMD64_PDF2ZH
            .pack_download_urls
            .iter()
            .any(|url| url.starts_with("https://github.com/")));
    }

    #[test]
    fn linux_profile_pins_the_release_pack() {
        assert!(LINUX_X64_PDF2ZH.enabled);
        assert_eq!(LINUX_X64_PDF2ZH.pack_directory_name, "linux-x64");
        assert_eq!(LINUX_X64_PDF2ZH.bin_relative_path, "bin/pdf2zh");
        assert_eq!(LINUX_X64_PDF2ZH.pack_size_bytes, Some(510_384_173));
        assert_eq!(
            LINUX_X64_PDF2ZH.pack_sha256,
            Some("4f71a0ea881f899d2c10a8a76874f453b4829840f8a1f36efcc19fde9bfd3f5d")
        );
        assert!(LINUX_X64_PDF2ZH
            .pack_download_urls
            .first()
            .is_some_and(|url| url.starts_with("https://githubdog.com/")));
        assert!(LINUX_X64_PDF2ZH
            .pack_download_urls
            .iter()
            .any(|url| url.starts_with("https://github.com/")));
    }

    #[test]
    fn current_profile_resolves_linux_x64() {
        if (std::env::consts::OS, std::env::consts::ARCH) == ("linux", "x86_64") {
            assert_eq!(
                current_profile().map(|profile| profile.id),
                Some("linux-x64-pdf2zh")
            );
        }
    }
}
