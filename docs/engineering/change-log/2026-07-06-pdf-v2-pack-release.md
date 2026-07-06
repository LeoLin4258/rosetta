# 2026-07-06 PDF v2 Component Pack Release

## Summary

Published the validated PDF Engine Contract v2 component packs to
`LeoLin4258/rosetta-assets` and updated Rosetta's managed PDF component
profiles to install them by default.

## Artifacts

Windows x64:

- Release tag: `pdf-layout-pack-windows-x64-v2026.07.06.1`.
- File: `rosetta-pdf2zh-windows-amd64.zip`.
- Size: `337538595` bytes.
- SHA256:
  `394bcfe73781f9098814b8ce9fd82cddbd9107831596c3d6353ce909fbd44bfd`.

macOS arm64:

- Release tag: `pdf-layout-pack-macos-arm64-v2026.07.06.1`.
- File: `rosetta-pdf2zh-macos-arm64.tar.gz`.
- Size: `384360401` bytes.
- SHA256:
  `60dff51fc3b3d336e9f068b747b3b7b5de86caca3adb44dd80068ef13c553e41`.

## Notes

- `managed_pdf2zh/profile.rs` now points both platforms at the v2 pack
  releases.
- The GitHubDog mirror URL remains first in each profile's
  `pack_download_urls`, with the original GitHub release URL as fallback.
- GitHub release asset metadata matched the local artifact size and SHA256
  values before the profile update.
