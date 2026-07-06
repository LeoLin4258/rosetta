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
    pack_size_bytes: Some(384_360_401),
    pack_sha256: Some("60dff51fc3b3d336e9f068b747b3b7b5de86caca3adb44dd80068ef13c553e41"),
    pack_download_urls: &[
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-macos-arm64-v2026.07.06.1/rosetta-pdf2zh-macos-arm64.tar.gz",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-macos-arm64-v2026.07.06.1/rosetta-pdf2zh-macos-arm64.tar.gz",
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
    pack_size_bytes: Some(337_538_595),
    pack_sha256: Some("394bcfe73781f9098814b8ce9fd82cddbd9107831596c3d6353ce909fbd44bfd"),
    pack_download_urls: &[
        "https://githubdog.com/https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.07.06.1/rosetta-pdf2zh-windows-amd64.zip",
        "https://github.com/LeoLin4258/rosetta-assets/releases/download/pdf-layout-pack-windows-x64-v2026.07.06.1/rosetta-pdf2zh-windows-amd64.zip",
    ],
};

const ALL_PROFILES: &[Pdf2zhProfile] = &[MACOS_ARM64_PDF2ZH, WINDOWS_AMD64_PDF2ZH];

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
    use super::{MACOS_ARM64_PDF2ZH, WINDOWS_AMD64_PDF2ZH};

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
}
