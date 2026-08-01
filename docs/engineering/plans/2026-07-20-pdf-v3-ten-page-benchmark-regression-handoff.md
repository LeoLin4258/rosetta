# 2026-07-20 PDF v3 Ten-Page Benchmark Regression Handoff

> **Archived / resolved.** This is failure and benchmark evidence, not an
> active handoff. Production returned to `pdf2zh` under ADR 0077. Continue from
> [`2026-07-27-pdf-stabilization-governance.md`](2026-07-27-pdf-stabilization-governance.md)
> and [`../pdf-pipeline.md`](../pdf-pipeline.md). See
> [`../archive/pdf-documents.md`](../archive/pdf-documents.md) for document status.

## Resolution

Resolved on 2026-07-21 by restoring pdf2zh as production extraction and visual
rendering authority, retaining the bounded native scheduler/store
infrastructure through an adapter, and optimizing the proven production path.

The exact ten-page fixture now completes cache-miss preparse in approximately
2.8-3.0 seconds in the App, translates in approximately 104-107 seconds with
the established 20 RWKV requests, produces ten compressed pages totaling about
4.3 MiB, and has passed the user's visual acceptance test.

Current status, final measurements, source-state warnings, and the deferred
resource-pack release checklist are recorded in:

```text
docs/engineering/plans/2026-07-21-pdf-production-refactor-closeout.md
```

The remainder of this document is retained as historical evidence of the
failed native production execution path. Its investigation instructions and
release-blocking status are superseded.

## Historical Status (Superseded)

Unresolved and release-blocking.

Do not continue broad PDF v3 feature work until the exact ten-page regression
below is understood. The current implementation passes the three-page Drylab
fixture, but fails the user's long-standing ten-page benchmark on both primary
product outcomes:

- end-to-end translation time is about twice the pre-rewrite baseline;
- the translated preview is almost entirely the original source PDF.

This is not a model-speed excuse. If the new PDF architecture creates more
model work, adds page barriers, wastes completed translations, or serializes
work that the old pipeline kept full, the resulting RWKV time is a PDF pipeline
regression.

## Non-Negotiable Testing Instruction

**Do not use AI-driven UI clicking as the primary reproduction or validation
method.** Computer Use, coordinate clicking, file-dialog automation and polling
the visible App are extremely slow and produced avoidable mistakes during this
work.

Use commands, direct harnesses, persisted job artifacts and focused tests:

- PowerShell for job/run/patch inspection;
- existing Node diagnostics and benchmark scripts;
- Cargo tests or an ignored command-driven integration probe for exact pages;
- direct PDF/PNG rendering with Poppler or PDFium;
- a dedicated command-line v3 run harness if the current repo lacks one.

UI testing should be limited to one final smoke check after command-level
acceptance passes. Do not spend investigation time simulating a user clicking
through import and translation.

## User Report

The user tested the PDF that has consistently served as the ten-page baseline:

```txt
C:\Users\Leo\Desktop\pdf-set-1\2604.17278v1.pdf
```

Observed:

- total time was roughly twice the pre-rewrite implementation;
- translated pages displayed the original text rather than translations;
- the user explicitly rejected attributing the regression to RWKV, because the
  old pipeline used the same local translation model and completed much faster.

Treat this report as authoritative product evidence. The persisted run confirms
both failures.

## Exact Failing Run

Environment:

- OS: Windows
- architecture: x86_64 on the user's AMD development machine
- branch: `codex/pdf-v3-rewrite`
- renderer: `rosetta-pdf-v3-region-translation-renderer/2`
- target language: `zh-CN`
- source fingerprint:
  `sha256:5db8200931a2d4104cf435a70701e80d47849c201000ed86ca645ab25d454da2`

Authority:

```txt
jobId: job-1784556548592-2604-17278v1
runId: run-pdf-v3-1784556552389-1-3
job root:
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1784556548592-2604-17278v1
```

Run manifest:

```txt
pdf-v3\runs\run-pdf-v3-1784556552389-1-3\manifest.json
```

Important manifest values:

```txt
requestedPageSet: 1-10
runState: completed
completedPages: 10
failedPages: 0
preservedPages: 0
maxExtractingPages: 2
maxExtractedPages: 4
maxTranslatingPages: 1
```

Run creation took only 130 ms, with model/font/sidecar digest cache hits. The
slowdown is not run creation or repeated model hashing.

The first scheduler authority timestamp was approximately
`1784556552389`; page 10 completed at `1784556814141`. Total run wall time was
therefore approximately:

```txt
261,752 ms = 4 minutes 21.8 seconds
```

## Pre-Rewrite Baseline Evidence

The same source fingerprint has historical pre-rewrite profiles under:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1784020720370-2604-17278v1\diagnostics
```

Recorded complete ten-page runs:

| Run | Total | RWKV request batches | Batch items | Average batch size |
| --- | ---: | ---: | ---: | ---: |
| `run-pdf-1784020732248` | 119,279 ms | 20 | 313 | 15.65 |
| `run-pdf-1784021374458` | 129,360 ms | 20 | 313 | 15.65 |
| `run-pdf-1784089504521` | 137,055 ms | 20 | 313 | 15.65 |

The new run is about 1.91x to 2.19x slower than those recorded baselines.

The historical profiles also show:

```txt
totalInputChars: 39,841
totalOutputChars: 13,430-13,651
renderCallCount: 10
```

Do not replace this evidence with a different fixture or a synthetic benchmark.
The exact source fingerprint must remain the primary performance gate.

## Why The Preview Shows Original Text

The new run did create region patches for all ten pages, so this is not simply a
missing translation-file or target-language selection problem.

However, patch inspection shows:

```txt
total flow containers: 157
reflowed containers: 5
preserved containers: 152
preserved ratio: 96.8%
```

Per page:

| Page | Containers | Reflowed | Preserved |
| ---: | ---: | ---: | ---: |
| 1 | 10 | 0 | 10 |
| 2 | 9 | 0 | 9 |
| 3 | 19 | 1 | 18 |
| 4 | 12 | 1 | 11 |
| 5 | 15 | 3 | 12 |
| 6 | 20 | 0 | 20 |
| 7 | 32 | 0 | 32 |
| 8 | 7 | 0 | 7 |
| 9 | 17 | 0 | 17 |
| 10 | 16 | 0 | 16 |

Preservation reasons:

| Count | Reason |
| ---: | --- |
| 70 | `region-source-ownership-incomplete` |
| 66 | `region-fit-bounds-unsupported` |
| 6 | `region-source-style-unsupported` |
| 4 | `region-layout-overflow` |
| 3 | `translation-fragmented-mixed-language` |
| 2 | `translation-appears-untranslated` |
| 1 | `translation-too-short` |

This explains the user-visible result: the renderer intentionally kept source
content in nearly every container. The scheduler nevertheless records every
page as `completed`, and the run summary records `preservedPages: 0` because
container preservation is not reflected in page/run success semantics.

This is a severe product bug. A page that spends model time translating its
content and then preserves all containers must not appear as successfully
translated.

## Why The New Run Is Slower

The persisted v3 patches contain 412 translated visual paragraphs:

| Page | Visual paragraphs |
| ---: | ---: |
| 1 | 15 |
| 2 | 13 |
| 3 | 18 |
| 4 | 13 |
| 5 | 15 |
| 6 | 61 |
| 7 | 235 |
| 8 | 11 |
| 9 | 16 |
| 10 | 15 |

Total translated output characters were about 13,561, close to the old
profiles. The model did not simply produce twice as much final text.

The strongest current hypotheses are:

1. The visual grouping path increased translation units from the old 313 batch
   items to 412 paragraphs, with page 7 exploding to 235 units.
2. `maxTranslatingPages: 1` introduces a hard page barrier. Even if one page
   cannot fill every local llama.cpp slot efficiently, the next page cannot use
   free capacity.
3. The old pipeline maintained wide batches across the complete document;
   the new page-local provider loop repeatedly drains and refills work.
4. Most model work is discarded after translation because 152/157 containers
   fail renderer ownership or fit preflight.
5. The new v3 run currently does not persist a complete per-page provider /
   validation / renderer timing profile, making regressions harder to attribute.

Do not assume all five are true. Measure them directly, but start here.

## Critical Semantic Failure

The current state model conflates these outcomes:

```txt
page translated and rendered
page completed with some source containers preserved
page completed with every meaningful container preserved
```

For this benchmark, pages 1, 2, 6, 7, 8, 9 and 10 had zero reflowed containers
yet were all stored as `completed`.

The next design must expose translated coverage as durable authority. At a
minimum, page/run status and diagnostics need bounded counts for:

```txt
eligible containers
translated/reflowed containers
preserved containers
translated source-character coverage
preservation reasons
```

Do not report a source-only page as translated success.

## What Is Still Reusable

The week of work is not entirely disposable. These components have focused
tests and can likely survive a rollback of the current execution strategy:

- page-local extraction and precise requested-page authority;
- bounded PageGraph and scheduler stores;
- durable compressed patch storage and cache identity;
- cancellation, lease, recovery and long-document bounded-state primitives;
- unified Source Han translation fonts and document-level subset reuse;
- typed renderer failure reasons;
- privacy-safe run creation diagnostics;
- native preview and export cache infrastructure.

The following must be considered provisional rather than protected:

- visual paragraph grouping as the provider execution unit;
- one-page-at-a-time translation scheduling;
- the current source-show ownership requirements;
- the current fit-bounds preflight for neutralization;
- container preservation being treated as page completion;
- claims that the region renderer is production-ready based on Drylab.

## Recent Fixes That Are Not The Benchmark Fix

The latest session fixed two real Drylab issues:

1. Empty source-show neutralization no longer requests or embeds a translation
   font. This fixed `pdf-v3-renderer-font-document-face-missing`.
2. Small mixed-scale/mixed-color decorative callouts are preserved instead of
   forced through body reflow. This fixed overlap in the Drylab `34 meetings`
   callout.

Renderer identity advanced to:

```txt
rosetta-pdf-v3-region-translation-renderer/2
```

The final three-page Drylab run completed successfully:

```txt
jobId: job-1784553544933-drylab
runId: run-pdf-v3-1784553721817-1-1
```

That result is valid for Drylab but does not mitigate the ten-page regression.

## RWKV Input Terminology

The user correctly emphasized that this dedicated RWKV translation model does
not accept instruction prompts or system prompts.

Rosetta calls llama.cpp `/completion`. The JSON field is technically named
`prompt`, but it contains only the model's fixed language-label serialization:

```txt
English: <source text>

Chinese:
```

Do not describe work on PDF paragraph reconstruction as “prompt engineering”.
The relevant variables are source-text cleanliness, segmentation, batching,
language-label serialization and generation settings. The latest session did
not change the fixed RWKV input template.

Relevant file:

```txt
rosetta-app/src-tauri/src/rwkv_providers/llama_cpp_chat.rs
```

## Command-First Investigation

### 1. Inspect exact old and new authorities

```powershell
$old = 'C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1784020720370-2604-17278v1'
$new = 'C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1784556548592-2604-17278v1'

Get-ChildItem "$old\diagnostics" -Filter 'pdf-translation-profile-*.json' |
  Sort-Object LastWriteTime |
  ForEach-Object { Get-Content -Raw $_.FullName }

Get-Content -Raw "$new\pdf-v3\runs\run-pdf-v3-1784556552389-1-3\manifest.json"
Get-Content -Raw "$new\pdf-v3\runs\run-pdf-v3-1784556552389-1-3\shard-00000000.json"
Get-Content -Raw "$new\diagnostics\pdf-timeline.jsonl"
```

### 2. Summarize region decisions without opening the App

Use PowerShell and `GZipStream` against:

```txt
$new\pdf-v3\translations\**\*.patch.json.gz
```

Group `payload.patch.containers[].rendererDecision.reason_code`, and report
reflowed/preserved counts by page. Never print source or translated paragraph
text into committed logs.

### 3. Measure provider work

If `ROSETTA_RWKV_IO_DEBUG=1` is enabled for a command-driven reproduction, use:

```powershell
cd rosetta-app
node scripts/benchmark-llama-cpp-pdf-debug.mjs `
  --log "$env:APPDATA\com.rosetta.desktop\logs\rwkv-io-debug.jsonl" `
  --context latest `
  --dry-run `
  --output "..\tmp\pdfs\ten-page-rwkv-summary.json"
```

`rwkv-io-debug.jsonl` may contain private document text. Do not commit it, paste
it into handoff notes, or send it externally.

### 4. Add or use a direct v3 harness

The existing v3 production entry is Tauri-command oriented. If there is no
command that can create and execute a run from a source path, make the first
investigation change a narrow local harness or ignored integration test that
accepts:

```txt
source path
page set
source language
target language
job/output root
provider endpoint
```

It must emit:

```txt
run id
per-page stage timings
provider request/unit counts
reflowed/preserved coverage
output page PDFs/PNGs
```

Use that harness for all iteration. Do not substitute UI automation.

### 5. Render output directly

Use the bundled Poppler executable or PDFium probes:

```powershell
& 'C:\Users\Leo\.cache\codex-runtimes\codex-primary-runtime\dependencies\native\poppler\Library\bin\pdftoppm.exe' `
  -png `
  '<translated-page-or-export.pdf>' `
  '<output-prefix>'
```

Inspect stable PNG files, not transient UI screenshots.

## Decision Gates

Do not keep patching the region renderer indefinitely.

After the command-level audit, choose explicitly:

### Keep and repair the region execution path only if

- ownership failures can be reduced to a small minority on the exact benchmark;
- fit-bounds failures are shown to be an implementation bug rather than a
  fundamental mismatch with real PDF content streams;
- global or cross-page provider scheduling can recover old throughput while
  page-local durable authority stays bounded;
- translated coverage can be measured and surfaced honestly.

### Roll back provider/render execution while retaining infrastructure if

- common academic/technical PDFs continue to preserve most containers;
- ownership proof requires document-producer-specific exceptions;
- provider throughput remains materially below the old pipeline after
  batching/concurrency restoration;
- the old renderer can be reattached to the new scheduler/store without losing
  long-document safety.

A rollback is acceptable. The App is beta and the user explicitly prefers a
clean, durable design over defending sunk cost.

## Acceptance Criteria

The exact ten-page source fingerprint must pass before calling PDF v3 usable:

1. Warm end-to-end time is no slower than the recorded pre-rewrite range. A
   practical initial gate is `<= 130 seconds`; explain any stricter target with
   measurements.
2. At least 95% of eligible translatable content is visibly translated, measured
   by durable character or container coverage. Do not count a source-preserved
   page as translated.
3. No page with zero translated/reflowed containers may report ordinary
   translated success.
4. Provider work must not be discarded after expensive translation when an
   equivalent renderer-safety decision could have been made before provider IO.
5. Per-page provider, validation, patch, render and preview timing must be
   available from commands.
6. The three-page Drylab regression must remain readable and preserve its
   decorative callout without overlap.
7. Focused PDF tests, `cargo test rosetta_jobs`, `cargo check` and frontend
   typechecking must pass.

Long-document 500/1,000-page memory, cancellation, recovery, cache and export
acceptance remains required after the ten-page gate is restored.

## Required Reading

Before changing architecture or data models:

```txt
docs/rosetta_project_plan.md
docs/engineering/README.md
docs/engineering/conventions/frontend.md
docs/engineering/conventions/data-models.md
docs/engineering/plans/2026-05-12-pdf-v1-support.md
docs/engineering/decisions/0076-pdf-v3-visual-paragraph-planning-and-flow-container-reflow.md
docs/engineering/change-log/2026-07-20-pdf-v3-region-renderer-production-switch.md
```

Key implementation files:

```txt
rosetta-app/src-tauri/src/pdf_v3/visual_grouping.rs
rosetta-app/src-tauri/src/pdf_v3/paragraph_translation_plan.rs
rosetta-app/src-tauri/src/pdf_v3/region_layout.rs
rosetta-app/src-tauri/src/pdf_v3/region_renderer.rs
rosetta-app/src-tauri/src/pdf_v3/replacement.rs
rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/unit_translation.rs
rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/v3_processor.rs
rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/v3_runtime.rs
```

Historical baseline tooling:

```txt
rosetta-app/scripts/check-pdf-translation-run.mjs
rosetta-app/scripts/benchmark-llama-cpp-pdf-debug.mjs
```

## Current Validation State

Before this handoff, the current worktree passed:

```txt
pnpm typecheck
cargo check
cargo test pdf_v3 --lib -- --nocapture
  214 passed, 25 ignored
cargo test rosetta_jobs --lib -- --nocapture
  131 passed
git diff --check
  no whitespace errors; Windows CRLF warnings only
```

These tests do not invalidate the ten-page failure. The missing acceptance test
is precisely the problem.

## Suggested Prompt For The Next Agent

```txt
接手 Rosetta PDF v3 十页基准回归。先阅读：

- docs/engineering/plans/2026-07-20-pdf-v3-ten-page-benchmark-regression-handoff.md
- docs/engineering/decisions/0076-pdf-v3-visual-paragraph-planning-and-flow-container-reflow.md
- docs/engineering/change-log/2026-07-20-pdf-v3-region-renderer-production-switch.md

核心证据：同一 10 页 `2604.17278v1.pdf`，旧链路 119-137 秒，新 `/2` 运行 261.8 秒。新运行 157 个 flow container 只回填 5 个，152 个保留原文，但 run 仍显示 10/10 completed。主要 preservation reason 是 `region-source-ownership-incomplete` 70 个和 `region-fit-bounds-unsupported` 66 个。

不要用 Computer Use、坐标点击或模拟用户操作作为主要测试方法。通过 PowerShell、Node 脚本、Cargo 测试和直接 PDF/PNG 渲染检查 job/run/patch；如果缺少直接执行 v3 的命令行 harness，先建立一个窄的本地 harness，再开始优化。

不要用“RWKV 时间占比高”解释回归。需要对比旧 313 batch items / 20 batches 与新 412 visual paragraphs、`maxTranslatingPages=1` 页面屏障、provider slot 利用率和翻译后被丢弃的 96.8% 容器。先做证据审计，再明确决定修复 region execution 还是回退执行层并保留 scheduler/store/font/cache 基础设施。
```
