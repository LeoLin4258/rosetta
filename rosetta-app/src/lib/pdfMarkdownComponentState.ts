export type PdfMarkdownComponentState =
  | "unsupported"
  | "not-installed"
  | "installed"
  | "needs-repair"
  | null;

export function pdfMarkdownNeedsPreparation(
  componentState: PdfMarkdownComponentState,
  extractionState: string | null | undefined,
): boolean {
  return componentState !== "installed" || extractionState !== "ready";
}
