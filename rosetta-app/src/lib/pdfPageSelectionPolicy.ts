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

export function shouldConfirmLongPdfTranslation(selectedPageCount: number) {
  return selectedPageCount > PDF_LONG_RANGE_CONFIRM_PAGE_LIMIT;
}
