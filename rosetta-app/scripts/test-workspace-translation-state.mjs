import assert from "node:assert/strict";

import {
  canExportSelectedTranslation,
  resolveTranslationProgress,
} from "../src/lib/workspaceTranslationState.ts";
import { pdfMarkdownNeedsPreparation } from "../src/lib/pdfMarkdownComponentState.ts";
import {
  isPreviewScrollKey,
  previewScrollMayDrive,
  previewScrollTargetChanged,
  proportionalPreviewScrollTop,
} from "../src/lib/previewScrollSync.ts";
import {
  resolveJobsPageSelection,
  sourceSelectionKey,
  translationSelectionKey,
} from "../src/lib/rosettaSelection.ts";

const restoredPartial = {
  completedSegments: 17,
  segmentCount: 419,
};

assert.deepEqual(
  resolveTranslationProgress(null, restoredPartial, { segmentCount: 419 }),
  { completed: 17, total: 419 },
  "restored Markdown progress must come from the durable translation file",
);
assert.deepEqual(
  resolveTranslationProgress(
    { completedSegmentIds: ["one", "two"], targetSegmentIds: ["one", "two", "three"] },
    restoredPartial,
    { segmentCount: 419 },
  ),
  { completed: 2, total: 3 },
  "an active run must remain the live progress authority",
);
assert.deepEqual(resolveTranslationProgress(null, null, { segmentCount: 12 }), {
  completed: 0,
  total: 12,
});
assert.deepEqual(resolveTranslationProgress(null, restoredPartial, null), {
  completed: 17,
  total: 419,
});
assert.equal(canExportSelectedTranslation(false, restoredPartial), false);
assert.equal(
  canExportSelectedTranslation(false, { completedSegments: 419, segmentCount: 419 }),
  true,
);
assert.equal(canExportSelectedTranslation(false, null), false);
assert.equal(
  canExportSelectedTranslation(false, { completedSegments: 0, segmentCount: 0 }),
  false,
);
assert.equal(canExportSelectedTranslation(true, restoredPartial), true);

assert.equal(
  pdfMarkdownNeedsPreparation("installed", "ready"),
  false,
  "a ready component with a ready extraction must expose translation actions",
);
assert.equal(
  pdfMarkdownNeedsPreparation("needs-repair", "ready"),
  true,
  "a broken component must expose repair even when an older extraction is ready",
);
assert.equal(
  pdfMarkdownNeedsPreparation("installed", "stale"),
  true,
  "a stale extraction must expose re-extraction",
);
assert.equal(pdfMarkdownNeedsPreparation(null, "ready"), true);
assert.equal(pdfMarkdownNeedsPreparation("unsupported", "ready"), true);
assert.equal(pdfMarkdownNeedsPreparation("installed", null), true);

assert.equal(
  proportionalPreviewScrollTop(
    { scrollTop: 450, scrollHeight: 1000, clientHeight: 100 },
    { scrollHeight: 1900, clientHeight: 100 },
  ),
  900,
  "paired previews must preserve proportional position across unequal heights",
);
assert.equal(
  proportionalPreviewScrollTop(
    { scrollTop: 2000, scrollHeight: 1000, clientHeight: 100 },
    { scrollHeight: 1900, clientHeight: 100 },
  ),
  1800,
  "scroll synchronization must clamp an overshooting source offset",
);
assert.equal(
  proportionalPreviewScrollTop(
    { scrollTop: 20, scrollHeight: 80, clientHeight: 100 },
    { scrollHeight: 300, clientHeight: 100 },
  ),
  0,
);
assert.equal(
  proportionalPreviewScrollTop(
    { scrollTop: -20, scrollHeight: 1000, clientHeight: 100 },
    { scrollHeight: 1900, clientHeight: 100 },
  ),
  0,
  "scroll synchronization must clamp a negative source offset",
);
assert.equal(previewScrollTargetChanged(100, 101.9), false);
assert.equal(previewScrollTargetChanged(100, 102), true);
assert.equal(previewScrollMayDrive(null, "source"), false);
assert.equal(previewScrollMayDrive("translation", "source"), false);
assert.equal(previewScrollMayDrive("source", "source"), true);
assert.equal(isPreviewScrollKey("PageDown"), true);
assert.equal(isPreviewScrollKey(" "), true);
assert.equal(isPreviewScrollKey("Enter"), false);

const job = {
  id: "job-pdf",
  sourceFiles: [{ id: "file-1", format: "pdf" }],
};
const markdownTranslation = {
  id: "tr-file-1-zh-cn-markdown",
  sourceFileId: "file-1",
  outputFormat: "markdown",
};
const persistedSelection = JSON.parse(
  JSON.stringify({
    activeOutputFormatBySourceKey: {
      [sourceSelectionKey(job.id, "file-1")]: "markdown",
    },
    activeTranslationFileIdBySourceKey: {
      [translationSelectionKey(job.id, "file-1", "markdown")]:
        markdownTranslation.id,
    },
  }),
);
const restoredSelection = resolveJobsPageSelection({
  activeDocument: null,
  activeJobId: job.id,
  activeSourceFileId: "file-1",
  activeSourceFileIdByJobId: { [job.id]: "file-1" },
  activeTranslationFileId: markdownTranslation.id,
  activeTranslationFileIdBySourceKey:
    persistedSelection.activeTranslationFileIdBySourceKey,
  activeOutputFormatBySourceKey:
    persistedSelection.activeOutputFormatBySourceKey,
  jobs: [job],
  translationFiles: [markdownTranslation],
});
assert.equal(restoredSelection.selectedOutputFormat, "markdown");
assert.equal(
  restoredSelection.selectedTranslationFile?.id,
  markdownTranslation.id,
  "serialized PDF/Markdown selection must resolve after restart",
);

console.log("Workspace translation state acceptance passed");
