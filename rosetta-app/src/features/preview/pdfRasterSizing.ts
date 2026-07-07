const PDF_PREVIEW_HORIZONTAL_PADDING = 32;
const PDF_PREVIEW_CHECKBOX_COLUMN = 32;
const PDF_PREVIEW_COLUMN_GAPS = 32;
const PDF_RASTER_MIN_WIDTH = 720;
const PDF_RASTER_MAX_WIDTH = 1200;
const PDF_RASTER_WIDTH_STEP = 64;
const PDF_RASTER_FALLBACK_WIDTH = 896;

export function pdfPreviewPaneWidth(viewportWidth: number) {
  if (!Number.isFinite(viewportWidth) || viewportWidth <= 0) {
    return 0;
  }

  return Math.max(
    (viewportWidth -
      PDF_PREVIEW_HORIZONTAL_PADDING -
      PDF_PREVIEW_CHECKBOX_COLUMN -
      PDF_PREVIEW_COLUMN_GAPS) /
      2,
    240,
  );
}

export function pdfRasterTargetWidth(viewportWidth: number, devicePixelRatio = 1) {
  const paneWidth = pdfPreviewPaneWidth(viewportWidth);
  if (paneWidth <= 0) {
    return PDF_RASTER_FALLBACK_WIDTH;
  }

  const ratio = Math.min(Math.max(devicePixelRatio || 1, 1), 2);
  const rawWidth = paneWidth * ratio;
  const steppedWidth =
    Math.ceil(rawWidth / PDF_RASTER_WIDTH_STEP) * PDF_RASTER_WIDTH_STEP;

  return Math.min(
    Math.max(steppedWidth, PDF_RASTER_MIN_WIDTH),
    PDF_RASTER_MAX_WIDTH,
  );
}
