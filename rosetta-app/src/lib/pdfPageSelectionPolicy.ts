export const PDF_AUTO_SELECT_ALL_PAGE_LIMIT = 30;
export const PDF_LONG_DOCUMENT_DEFAULT_SELECTION = 10;
export const PDF_LONG_RANGE_CONFIRM_PAGE_LIMIT = 50;

export function normalizePdfPageNumbers(
  pages: number[],
  pageCount?: number | null,
) {
  const maxPage = pageCount && pageCount > 0 ? pageCount : Number.POSITIVE_INFINITY;
  return [...new Set(pages)]
    .filter((page) => Number.isInteger(page) && page > 0 && page <= maxPage)
    .sort((left, right) => left - right);
}

export function defaultPdfSelectedPages(pageCount: number) {
  if (pageCount <= 0) return [];
  const selectedCount =
    pageCount <= PDF_AUTO_SELECT_ALL_PAGE_LIMIT
      ? pageCount
      : Math.min(PDF_LONG_DOCUMENT_DEFAULT_SELECTION, pageCount);
  return Array.from({ length: selectedCount }, (_, index) => index + 1);
}

export function recommendedPdfSelectedPages(
  pageCount: number,
  completedPages: number[],
) {
  const defaults = defaultPdfSelectedPages(pageCount);
  const completed = new Set(normalizePdfPageNumbers(completedPages, pageCount));
  if (completed.size === 0 || completed.size >= pageCount) return defaults;

  const remaining = Array.from({ length: pageCount }, (_, index) => index + 1)
    .filter((page) => !completed.has(page));

  return pageCount <= PDF_AUTO_SELECT_ALL_PAGE_LIMIT
    ? remaining
    : remaining.slice(0, PDF_LONG_DOCUMENT_DEFAULT_SELECTION);
}

export function nextPdfSelectedPages(
  pageCount: number,
  currentSelection: number[],
  completedPages: number[],
) {
  const selected = normalizePdfPageNumbers(currentSelection, pageCount);
  if (selected.length === 0) {
    return recommendedPdfSelectedPages(pageCount, completedPages);
  }

  const completed = new Set(normalizePdfPageNumbers(completedPages, pageCount));
  if (!selected.every((page) => completed.has(page))) return selected;

  const remaining = Array.from({ length: pageCount }, (_, index) => index + 1)
    .filter((page) => !completed.has(page));
  if (remaining.length === 0) return selected;

  const lastSelectedPage = selected[selected.length - 1];
  const pagesAfterSelection = remaining.filter((page) => page > lastSelectedPage);
  const pagesBeforeSelection = remaining.filter((page) => page <= lastSelectedPage);
  const batchSize = selected.length;

  return [...pagesAfterSelection, ...pagesBeforeSelection].slice(0, batchSize);
}

export function pdfPageSelectionLabel(pages: number[], pageCount: number) {
  const selected = normalizePdfPageNumbers(pages, pageCount);
  if (selected.length === 0) return "未选择页面";
  if (pageCount > 0 && selected.length === pageCount) return `全部 ${pageCount} 页`;

  const firstPage = selected[0];
  const lastPage = selected[selected.length - 1];
  const contiguous = selected.every(
    (page, index) => page === firstPage + index,
  );
  if (contiguous) {
    return firstPage === lastPage
      ? `第 ${firstPage} 页`
      : `第 ${firstPage}-${lastPage} 页`;
  }

  return `所选 ${selected.length} 页`;
}

export function shouldConfirmLongPdfTranslation(selectedPageCount: number) {
  return selectedPageCount > PDF_LONG_RANGE_CONFIRM_PAGE_LIMIT;
}
