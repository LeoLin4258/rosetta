# 2026-07-07 PDF Windows Font Weight Parity

## Original Goal

This work started from a PDF translation parity problem between macOS and
Windows.

The user observed that macOS translated PDFs could match the source PDF's font
weight more closely, while Windows translated PDFs did not. The initial request
was to inspect whether there were other macOS/Windows differences in the PDF
translation path and propose a plan to align both platforms.

During the same investigation, the user clarified two important product
requirements:

- background PDF page artifact compression must not be Windows-only; it was
  originally added because translated page artifacts could become extremely
  large, and that disk-pressure problem exists on both platforms;
- translated PDF font weight must preserve source-like emphasis without making
  Chinese text look much heavier than the original PDF.

## Issues Found

### Windows translated text looked too bold

The installed pdf2zh pack patch was simulating bold translated text with PDF
text rendering mode stroke (`w 2 Tr`). On Windows this made simplified Chinese
translations look much heavier than the source bold text.

### Removing faux bold made output look unbolded until the page was regenerated

After the faux-bold stroke was removed and simplified Chinese output was moved
from Source Han Serif to Source Han Sans Regular, the user tested a translated
PDF and reported that bold emphasis seemed to disappear.

The root cause for that test result was not the final font-switching patch. The
page artifact being viewed had not been regenerated with the updated pack:

```txt
translated-pages/zh-CN/page-0006.pdf
/notobold: 0
/noto: 92
w 2 Tr: 0
font: Source Han Sans CN Regular only
```

Once Rosetta performed a real forced retranslation after the worker was ready,
the regenerated page used both regular and bold simplified Chinese fonts.

### PDF translated artifacts were large because of embedded CJK fonts

The huge translated single-page artifacts were not caused by the app UI font.
They were caused by PDF page artifacts retaining full embedded CJK font copies.
The background compression task uses PyMuPDF font subsetting and object cleanup
to reduce those files after the translation hot path commits page artifacts.

## Completed Work

### Cross-platform PDF page artifact compression

The background compression path was made cross-platform:

- added a platform-aware PDF pack Python path helper;
- switched compression to use the installed pack Python path instead of the
  Windows profile binary path;
- removed the Windows-only guard so macOS and Windows can both run page artifact
  compression when an installed PDF component pack is available;
- kept `ROSETTA_PDF_PAGE_ARTIFACT_COMPRESSION=off` as the local diagnostic
  switch.

This is documented in:

```txt
docs/engineering/change-log/2026-07-07-pdf-artifact-compression-cross-platform.md
docs/engineering/pdf-pipeline.md
```

### Replaced faux bold with real font switching

The pdf2zh pack patch now avoids PDF text stroke for translated text:

```txt
rosetta_pdf_text_mode_operator(...) -> "0 Tr "
```

For simplified Chinese PDF output:

- normal translated Chinese uses BabelDOC's
  `SourceHanSansCN-Regular.ttf`;
- translated paragraphs whose source paragraph contains a bold, medium, demi,
  semibold, black, heavy, or `Bd` font run use
  `SourceHanSansCN-Bold.ttf`;
- the bold font is registered as a separate PDF font resource named
  `notobold`;
- bold detection is cumulative across the paragraph, so a later regular source
  run cannot erase an earlier bold/medium source run;
- this uses BabelDOC-bundled font assets instead of platform system fonts, so
  macOS and Windows follow the same behavior.

Primary files:

```txt
rosetta-app/src-tauri/scripts/patch-pdf2zh-color-preservation.py
rosetta-app/src-tauri/scripts/test-pdf2zh-patches.py
docs/engineering/pdf-pipeline.md
docs/engineering/change-log/2026-06-03-pdf-color-preservation.md
```

### Applied and verified on the local Windows pack

The updated patch was applied to the local installed Windows pdf2zh pack:

```powershell
$py = Join-Path $env:LOCALAPPDATA 'com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\python\python.exe'
& $py rosetta-app\src-tauri\scripts\patch-pdf2zh-color-preservation.py
```

The stale pdf2zh worker was restarted so the Tauri dev app loaded the updated
pack code.

### Forced a real Windows retranslation and verified the PDF structure

The user had Rosetta running through `pnpm tauri dev`. A forced retranslation of
the 10-page `2604.17278v1.pdf` job was triggered in the UI after the PDF engine
was ready.

One first attempt failed while the worker was restarting:

```txt
PDF 译文生成失败：写入 worker 任务失败: 管道正在被关闭。 (os error 232)
```

After the worker reported ready, the forced retranslation completed. The
regenerated page 6 artifact was:

```txt
C:\Users\Leo\AppData\Roaming\com.rosetta.desktop\jobs\job-1783068328587-2604-17278v1\translated-pages\zh-CN\page-0006.pdf
```

Structural verification after compression:

```txt
/notobold: 17
/noto: 96
w 2 Tr: 0
0 Tr: 633
font: Source Han Sans CN Regular, resource noto
font: Source Han Sans CN Bold, resource notobold
```

Rendered verification image:

```txt
C:\Users\Leo\Documents\GitHub\rosetta\tmp\pdf-bold-current\translated-page6-1.png
```

Visual result: section headings, emphasized paragraph starts, and bold table
rows are distinguishable, while body text no longer has the old heavy faux-bold
stroke.

## Validation

Patch tests:

```powershell
python rosetta-app\src-tauri\scripts\test-pdf2zh-patches.py
```

Result:

```txt
Ran 6 tests in 0.749s
OK
```

Rust PDF/job tests:

```powershell
cd rosetta-app\src-tauri
cargo test rosetta_jobs
```

Result:

```txt
63 passed; 0 failed
```

## Current State

The specific Windows PDF font-weight issue is resolved locally:

- no faux-bold PDF text stroke is used;
- simplified Chinese output uses Source Han Sans Regular/Bold;
- regenerated Windows PDF artifacts now contain `notobold`;
- visual output shows source-like emphasis without the earlier overweight text.

The broader PDF parity backlog remains future work where not already addressed:

- consolidate duplicated pack patch logic across macOS, Windows, and local
  staging scripts;
- align PDF-related requirement pins where practical;
- add fixture render smoke tests for bold/color preservation;
- add a release-time two-platform PDF parity report.
