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
  const extension = format === "markdown" ? "md" : "txt";
  const filename = relativePath.split(/[\\/]/).pop() ?? relativePath;
  const baseName = filename.replace(/\.(txt|md|markdown|pdf)$/i, "");
  const suffix = kind === "bilingual" ? `${targetLang}.bilingual` : targetLang;
  return `${baseName}.${suffix}.${extension}`;
}

export function exportFormatForSource(
  format: RosettaSourceDocumentFormat
): "txt" | "markdown" {
  return format === "markdown" ? "markdown" : "txt";
}
