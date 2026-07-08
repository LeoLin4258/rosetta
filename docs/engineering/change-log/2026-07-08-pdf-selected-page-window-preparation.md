# 2026-07-08 PDF Selected Page Window Preparation

## Summary

Improved PDF translation performance for large source PDFs when the user only
translates a small page selection.

The SpaceX prospectus regression showed that translating pages 1-10 of a
400-page PDF spent about 31.7 seconds in PDF warmup/preparation before model
translation began. The Python `pdf2zh.rosetta_engine` path accepted selected
pages, but it still opened, font-patched, and saved a prepared copy of the
entire source PDF.

## Change

- The pdf2zh pack patch now updates `rosetta_engine.prepareRun` to read the
  original source page count first, normalize the requested page selection
  against that count, and then build a prepared document containing only the
  selected pages.
- The prepared window keeps original Rosetta page numbers for job state,
  artifacts, diagnostics, and export behavior.
- Internally, pdfminer/layout/render now use the prepared-window page index
  instead of the original source page index.
- Single-page artifacts remain named by original page number, for example a
  one-page prepared window for source page 10 still renders
  `page-0010.pdf`.

## Local Verification

The local Windows PDF component pack was patched in place:

```powershell
$py = Join-Path $env:LOCALAPPDATA 'com.rosetta.desktop\pdf2zh-sidecar\pack\windows-amd64\python\python.exe'
& $py rosetta-app\src-tauri\scripts\patch-pdf2zh-color-preservation.py
```

SpaceX pages 1-10 prepare smoke after the patch:

```txt
sourcePageCount: 400
selectedPages: [1,2,3,4,5,6,7,8,9,10]
preparedDocPageCount: 10
elapsedMs: 10756
unitCount: 158
sourceChars: 47521
```

Single selected-page render smoke for source page 10:

```txt
sourcePageCount: 400
preparedDocPageCount: 1
pageNumber: 10
artifactExists: true
sourceUnitCount: 16
translatedUnitCount: 16
```

This primarily reduces fixed PDF preparation cost for large source documents.
It does not remove model-time cost from genuinely dense pages or from llama.cpp
split retries.

## Validation

```powershell
python rosetta-app\src-tauri\scripts\test-pdf2zh-patches.py
```

Result:

```txt
10 passed
```

