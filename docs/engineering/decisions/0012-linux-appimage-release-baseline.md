# ADR 0012: Linux x64 AppImage Release Baseline

Date: 2026-07-14

## Status

Accepted for preview packaging

## Context

Rosetta's Linux x64 application, managed RWKV runtime, PDF component, and real
PDF translation workflow have passed development testing on Ubuntu 24.04. The
project still needs a stable application artifact before the updater service
and website can expose Linux downloads.

Linux packaging choices have different update behavior. A Debian package is
familiar on Ubuntu, but Tauri's Linux self-updater is designed around AppImage.
Maintaining both formats in the first release would create two installation
contracts, only one of which supports the required in-app update workflow.

AppImage bundles most application libraries but not glibc. The available native
build and test host is Ubuntu 24.04 with glibc 2.39, so artifacts produced there
cannot honestly claim compatibility with older distributions without a separate
older-baseline build and test environment.

## Decision

- Use AppImage as the first Linux x64 application distribution format.
- Use the executable `.AppImage` as the future website download artifact.
- Use a `.AppImage.tar.gz` made from the same AppImage plus a Tauri updater
  signature as the future in-app update artifact.
- Reuse the existing cross-platform Tauri updater keypair. Do not generate a
  Linux-specific keypair.
- Build Linux artifacts natively on Linux x64. Do not cross-package them from
  Windows or macOS.
- Initially support Ubuntu 24.04 or newer on x86_64. Expanding support to Ubuntu
  22.04 or other older distributions requires building on an older glibc
  baseline and repeating the full runtime, PDF, and updater smoke tests.
- Keep managed RWKV runtimes, models, and the PDF component outside the
  AppImage. The application downloads checksum-pinned platform resources through
  the existing managed component flows.
- Real release builds require a clean worktree and a Tauri updater signature.
  An explicit unsigned preview mode may be used for local packaging tests but
  must not be uploaded or published.

## Consequences

- Linux users receive one installation format with a consistent future update
  path.
- The first Linux public release will not provide `.deb`, RPM, ARM64, Flatpak,
  or Snap packages.
- The website and Supabase release service can be extended after the AppImage
  passes launch and translation testing.
- Ubuntu 22.04 compatibility remains a future release-engineering task rather
  than an untested claim.
