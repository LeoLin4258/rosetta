# PDFMathTranslate Production Preparse Optimization

Date: 2026-07-21

## Scope

This benchmark covers CPU and storage optimizations in the production
`pdf2zh` path. The unit collector, unit IDs, layout masks, renderer semantics,
Rust/RWKV batching, prompts, concurrency, and translation request behavior are
unchanged.

The retained optimizations are:

- reuse one pdfminer `PDFResourceManager` per prepared run;
- replace scalar NumPy coordinate clamps in character hot loops with integer
  `min`/`max` clamps;
- canonicalize duplicate-layer text once per page and use exact
  `SequenceMatcher` upper bounds before the full ratio calculation;
- create the regular and bold output font objects once per document and share
  their xrefs across page resources;
- subset fonts in durable single-page artifacts before saving, with a
  best-effort fallback for unsupported fonts.

## Fixture and Environment

- Source: `2604.17278v1.pdf`, 10 pages.
- Fork: `PDFMathTranslate` commit `990bed0` plus the local changes.
- Installed pack: Windows amd64 Rosetta pdf2zh component pack.
- Layout model: `doclayout_yolo_docstructbench_imgsz1024.onnx`.
- Engine call: `prepareRun(pages=1..10, langIn=en, langOut=zh)`.
- Each timed sample used a fresh Python process and prewarmed the ONNX model.

## Cumulative Preparse Result

The exact installed pre-optimization engine and converter backups were
interleaved with the final installed versions:

| Sample | Before all changes | After all changes |
|---|---:|---:|
| 1 | 8,299.7 ms | 6,484.3 ms |
| 2 | 7,473.7 ms | 4,335.4 ms |
| 3 | 6,801.0 ms | 4,321.0 ms |
| Median | 7,473.7 ms | 4,335.4 ms |

The median improvement is 3,138.3 ms, or 42.0%. All six samples preserved:

- 105 collected units;
- 96 translatable units;
- 41,083 translatable source characters;
- serialized unit payload SHA-256
  `1112ac7b7b4509d7a447c31df8ea8b5a4c964e5ae69b304ca80a93ef7c128c42`.

The first final-version sample shows substantial host timing variance. The
median is used instead of the fastest sample.

## Focused A/B Results

These focused runs isolate each retained preparse optimization. Their clocks
were recorded at different points in the session and must not be added.

| Optimization | Before | After | Change |
|---|---:|---:|---:|
| pdfminer resource manager reuse, average | 8,496 ms | 7,380 ms | -13.1% |
| Scalar coordinate clamps, median | 6,156.9 ms | 5,085.2 ms | -17.4% |
| Duplicate-layer matching, median | 5,035.9 ms | 4,736.8 ms | -5.9% |
| Shared document font objects, median | 4,715.0 ms | 4,230.3 ms | -10.3% |

After the duplicate-layer change, profiled duplicate detection fell from about
0.78 seconds to 0.15 seconds. A final profile still attributes roughly three
seconds to ONNX layout inference, which is now the dominant cache-miss floor.

ONNX page batching was tested and rejected because batches of 2, 5, and 10
were unchanged or slower on the installed CPU provider. Lower layout input
resolutions were also rejected because most tested values changed unit
partitioning; the one exact fixture result saved only about 0.16 seconds.

## Artifact Storage Result

The production engine rendered all ten pages with source text substituted for
translations. Default lightweight font subsetting changed total durable page
artifact size as follows:

| Measurement | Before | After | Change |
|---|---:|---:|---:|
| Immediate ten page artifacts | 209,914,018 bytes | 139,294,288 bytes | -33.6% |
| After existing background compression | 209,914,018 bytes | 91,440,222 bytes | -56.4% |
| Complete prepare and render replay | 7,796.6 ms | 7,404.0 ms | no observed regression |

A separate post-processing probe measured about 0.59 seconds for subsetting
and object cleanup across all ten pages. The existing post-translation
background compressor then performs full stream deflation without delaying
RWKV requests. Running it on the already-subset artifacts took about 4.8
seconds and reduced the ten pages to 91.4 MB. Inline full deflation remains
disabled because it would add that work to the page render path.

## Windows DirectML Layout Result

The remaining ONNX hotspot was split into image rasterization, input
conversion, inference, and postprocessing. CPU ONNX inference accounted for
2,690 ms of a 2,923 ms layout pass; rasterization, conversion, and mask
construction were not the bottleneck.

The Windows component now uses ONNX Runtime DirectML with a bounded five-page
batch. Pages with different tensor shapes are grouped separately, page order
is restored before unit collection, and initialization or inference failure
falls back to the original CPU provider. macOS and Linux retain their existing
single-page provider behavior.

| Measurement | CPU | DirectML batch 5 | Change |
|---|---:|---:|---:|
| Ten-page layout | 2,923.0 ms | 1,388.9 ms | -52.5% |
| Complete warm `prepareRun`, median | 4,335.4 ms | 2,529.2 ms | -41.7% |

The cumulative warm `prepareRun` reduction from the pre-optimization median is
7,473.7 ms to 2,529.2 ms, or 66.2%. A non-prewarmed process showed a smaller
6,293 ms to 5,465 ms improvement because DirectML session creation and first
shader compilation remain cold-start work. Rosetta prewarms the managed PDF
worker at App startup; immediate-import behavior remains a manual product gate.

The DirectML and CPU masks were array-identical on all ten pages. Both paths
also preserved the exact production authority:

- 105 collected units;
- 96 translatable units;
- 41,083 translatable source characters;
- serialized unit payload SHA-256
  `1112ac7b7b4509d7a447c31df8ea8b5a4c964e5ae69b304ca80a93ef7c128c42`.

CPU and DirectML source-substitution renders produced identical artifact byte
totals and 10/10 pixel-identical Poppler PNG pairs. Sparse page preparation for
pages 2, 5, and 10 also produced the same unit hash before and after a durable
layout-cache restore.

The batch is capped at five pages, so its memory cost does not grow with
document length. On this Windows fixture, peak PDF worker RSS was approximately
497 MB for CPU and 1,367 MB for DirectML batch 5. Batch 2 reduced the DirectML
peak to approximately 703 MB but did not retain the required large speedup. A
fixed-token RWKV contention probe produced median throughput of 217.5 predicted
tokens/s without a resident DirectML prepare state and 222.0 predicted tokens/s
with it. One DirectML-resident sample was materially slower, so a complete App
translation remains required to reject model-speed regression under real
scheduling.

## Visual Regression

Every retained optimization was replayed through the production renderer.
Poppler rasterization at 96 DPI produced identical PNG hashes for all 10/10
before/after page pairs. Pages 1 and 10 were also visually inspected. Poppler
reported the same missing display-font warnings on both sides.

## Validation

```text
PDFMathTranslate test/test_rosetta_engine.py test/test_converter.py -q
25 passed

Rosetta test-pdf2zh-patches.py -q
33 passed
```

Application typechecking and Rust validation are recorded separately after the
final source changes. Product acceptance remains the user's manual test.
