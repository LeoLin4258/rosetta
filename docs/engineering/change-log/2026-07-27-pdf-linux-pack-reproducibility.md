# 2026-07-27 PDF Linux Pack Reproducibility and Runtime Diet

## Summary

Locked the Linux x64 PDF pack dependency graph and external build inputs so a
release artifact can be traced to one complete build recipe.

## Changes

- Added a `uv 0.11.32`-generated Python 3.12/Linux x64 dependency lock with
  hashes for all 104 distributions and binary-only resolution.
- Made the Linux pack install the lock with required hashes, binary wheels, and
  no dependency re-resolution.
- Pinned and verified the Python runtime archive, DocLayout model, BabelDOC
  fonts, and PDFMathTranslate commit before use.
- Added a machine-readable external-input manifest and a build recipe identity
  that binds the Rosetta commit and all release recipe file hashes.
- Embedded the dependency lock, normalized freeze, input manifest, and build
  recipe in the pack while retaining sidecar copies for release evidence.
- Persisted complete build stdout and stderr in the distribution directory.
- Made the PDF patcher accept both supported upstream text-output guard forms;
  this preserves the existing authoritative render-slot behavior for the
  pinned PDFMathTranslate source.

No persistent data format or product workflow changed.

## Runtime Diet

- Proved with import-time tracing and reverse dependency inspection that Azure,
  DeepL, Ollama, OpenAI, Tencent, and Xinference are eagerly imported by the
  pinned PDFMathTranslate translator module. Their SDKs remain in the pack;
  removing them requires upstream import decoupling rather than fake stubs.
- Removed the standalone runtime headers and static libraries, Tcl/Tk/IDLE,
  package test directories, and Python console scripts that the Rosetta worker
  never invokes. The pack keeps only the Python interpreter entry points.
- Re-ran the real prepare, collect, and render smoke after pruning so a removed
  file cannot pass only the pre-prune checks.
- Added a deterministic CycloneDX SBOM and license inventory. The generator
  verifies every declared Python distribution license file still exists and
  records hashes for CPython, licenses, fonts, and the layout model. The font
  OFL and model Apache license texts are themselves hash-locked build inputs.
- Kept the duplicate OpenCV distributions because the locked BabelDOC and
  PDFMathTranslate metadata require both variants and the installed `cv2`
  binary currently binds the GUI wheel. Changing that resolution belongs with
  dependency/import decoupling, not content deletion.

## Validation

- Two isolated Ubuntu x64 builds produced identical normalized freezes and
  identical unpacked content inventories: 1,354,528,694 bytes, 21,792 regular
  files, 5,077 directories, and 1,048 symlinks.
- Both in-place and relocated real-PDF pack smoke tests passed.
- The ten-page CP0 fixture reproduced 94 translation units, 41,035 source
  characters, the frozen unit hash, 10 successful page renders, and the frozen
  identity artifact byte count.
- The PDF patch suite passed all 39 tests.

The two compressed archive hashes differ because tar and gzip metadata are not
yet normalized. CP2 compares the dependency freeze and unpacked inventory;
byte-identical archive generation remains separate release hardening work.

The CP3 Linux candidate is 475,184,227 compressed bytes and 1,262,340,076
unpacked bytes, with 11,103 regular files and 1,044 symlinks. Relative to the
CP2 candidate, conservative pruning removes 35,298,038 compressed bytes and
92,188,618 unpacked bytes. The ten-page fixture remains at 94 units, 41,035
source characters, unit SHA-256
`81d6185ffc72f263bbc03a6ab1872e4e8615728ad47ecd359b1b2b1d2f3cecb5`,
and 139,293,175 identity-render artifact bytes. The final SBOM covers 105
installed distributions and the license inventory retains 196 license files.

## Linux Preparse Runtime Stabilization

User acceptance exposed a Linux-only ONNX Runtime CPU scheduling cliff. The
slow ten-page UI samples spent 8.1–10.1 seconds in layout inference while the
worker accumulated 331.9 CPU-seconds and exposed 32 runnable threads; the rest
of the application cgroup accumulated only 2.6 CPU-seconds during the same
sample. Pack I/O, persistent layout caching, preview rasterization, memory
pressure, CPU frequency, and RWKV contention were excluded on the same host.

The managed Linux worker now replaces the automatic Azure-plus-CPU layout
session with a CPU-only session and sets ONNX Runtime intra-op concurrency to
the physical cores visible in the worker's CPU affinity. GPU-backed provider
paths on Windows and macOS are unchanged. The redundant per-cache-miss
synthetic layout wakeup was removed.

On the 16-core/32-thread Linux acceptance host, explicit 16-core inference
reduced standalone ten-page prepare from 4.28 seconds to 3.54–3.62 seconds.
Two fresh jobs imported through the production UI completed in 3.847 and
3.801 seconds. Both retained 94 units, 41,035 source characters, and canonical
unit SHA-256
`81d6185ffc72f263bbc03a6ab1872e4e8615728ad47ecd359b1b2b1d2f3cecb5`.

## Diagnostic Privacy Hardening

The opt-in Linux preparse diagnostics now emit only bounded operational fields:
job-local request ID, page count, language direction, thread/layout settings,
CPU affinity, process ID, and timing counters. They no longer serialize the
worker request or normalized options, which contained source, output, scratch,
and persistent-cache paths plus the source fingerprint.

Rust and Python now use the same explicit truthy values (`1`, `true`, `yes`,
and `on`) for `ROSETTA_PDF_DIAGNOSTICS`. Values such as `0`, `false`, and `off`
do not enable request diagnostics. Focused tests enforce both the flag behavior
and the absence of raw request/options serialization in the embedded worker.

## Installer Safety Boundaries

- PDF pack downloads now have a 650 MiB absolute ceiling. A pinned-size download
  is stopped before writing the chunk that would exceed the expected size plus
  a 64 KiB protocol tolerance; the final artifact must still match the exact
  pinned size and SHA-256.
- Installation preflight checks free space for the archive, extraction staging,
  the new pack, and a 256 MiB safety margin while retaining the current pack.
  The candidate and final pack share one filesystem and are renamed rather than
  copied, so they do not require two simultaneous unpacked copies.
- ZIP and tar.gz extraction now use bounded Rust readers. They reject traversal,
  duplicate paths, escaping links, paths that traverse archive symlinks,
  excessive unpacked bytes, file/symlink counts, and oversized single files.
  The tar reader checks cancellation on every underlying read instead of only
  before starting the platform `tar` process.
- Upgrades rename the current pack to a sibling backup, activate and finalize
  the candidate, and delete the backup only after bytecode cleanup and an
  atomic installed-manifest replacement succeed. Failure or cancellation
  restores both the previous pack and manifest; fresh-install failure removes
  the incomplete candidate and manifest.
- Installed pack manifest schema 2 records `unpackedSizeBytes`, `fileCount`,
  `symlinkCount`, and `maxSingleFileBytes`. Schema 1 remains readable and keeps
  its existing identity/capability compatibility behavior. Future release
  sidecars and local staging manifests emit the same capacity evidence.

## CP11 Release Candidate

- Generated a Linux RC from the pinned dependency lock and external inputs with
  complete manifest, inventory, size gate, SBOM, license, freeze, checksum, and
  build-log evidence. The archive is 475,162,678 bytes and remains within every
  accepted Linux budget.
- Fixed the release builder's maximum-file calculation so `pipefail` cannot
  terminate a successful build after archive creation but before manifest and
  checksum publication.
- Added an explicit ignored release-gate test that extracts the real RC under
  installer limits, performs a fresh activation, and upgrades a real
  2026-07-15 archive without leaving a backup directory.
- Isolated page-artifact compression from AppImage-injected Python environment
  variables. A real RC AppImage compressed all ten accepted translated pages
  without failures; independent reopening confirmed all ten artifacts remained
  single-page PDFs with text.

User visual acceptance passed against the isolated RC pack and rebuilt Linux
AppImage. The immutable asset upload, Linux CI for the final committed source,
and Linux profile update remain intentionally pending.
