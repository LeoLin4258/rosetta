import type {
  RosettaExportKind,
  RosettaSourceDocumentFormat,
} from "@/types/rosetta";

export function defaultExportFilename(
  relativePath: string,
  format: RosettaSourceDocumentFormat,
  targetLang: string,
  kind: RosettaExportKind
) {
  // PDF documents always export back to PDF (we copy the cached translated
  // PDF directly). Bilingual side-by-side PDF isn't supported in v1 — see
  // `handleExport` in WorkspacePage for the dispatch.
  const extension =
    format === "pdf" ? "pdf" : format === "markdown" ? "md" : "txt";
  const filename = relativePath.split(/[\\/]/).pop() ?? relativePath;
  const baseName = filename.replace(/\.(txt|md|markdown|pdf)$/i, "");
  const suffix = kind === "bilingual" ? `${targetLang}.bilingual` : targetLang;
  return `${baseName}.${suffix}.${extension}`;
}

export function exportFormatForSource(
  format: RosettaSourceDocumentFormat
): "txt" | "markdown" | "pdf" {
  if (format === "markdown") return "markdown";
  if (format === "pdf") return "pdf";
  return "txt";
}
