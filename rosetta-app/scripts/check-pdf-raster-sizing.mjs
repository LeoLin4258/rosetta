import assert from "node:assert/strict";

import {
  pdfRasterTargetWidth,
  pdfPreviewPaneWidth,
} from "../src/features/preview/pdfRasterSizing.ts";

assert.equal(pdfPreviewPaneWidth(1200), 552);
assert.equal(pdfRasterTargetWidth(1200, 2), 1152);
assert.equal(pdfRasterTargetWidth(900, 2), 832);
assert.equal(pdfRasterTargetWidth(1600, 2), 1200);
assert.equal(pdfRasterTargetWidth(0, 2), 896);

console.log("pdf raster sizing checks passed");
