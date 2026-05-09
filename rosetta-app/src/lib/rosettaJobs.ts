import { invoke } from "@tauri-apps/api/core";
import type {
  RosettaExportKind,
  RosettaExportResult,
  RosettaJobFileDeleteResult,
  RosettaJobBundle,
  RosettaJobSummary,
  Segment,
  TranslationRevisionReason,
} from "../types/rosetta";

export function importRosettaDocumentFromPath(path: string) {
  return invoke<RosettaJobBundle>("import_rosetta_document_from_path", { path });
}

export function importRosettaProjectFromDirectory(path: string) {
  return invoke<RosettaJobBundle>("import_rosetta_project_from_directory", {
    path,
  });
}

export function pickRosettaImportPath() {
  return invoke<string | null>("pick_rosetta_import_path");
}

export function pickRosettaImportDirectory() {
  return invoke<string | null>("pick_rosetta_import_directory");
}

export function pickRosettaExportPath(
  defaultFilename: string,
  format: "txt" | "markdown"
) {
  return invoke<string | null>("pick_rosetta_export_path", {
    defaultFilename,
    format,
  });
}

export function listRosettaJobs() {
  return invoke<RosettaJobSummary[]>("list_rosetta_jobs");
}

export function loadRosettaJob(jobId: string) {
  return invoke<RosettaJobBundle>("load_rosetta_job", { jobId });
}

export function saveRosettaSegments(jobId: string, segments: Segment[]) {
  return invoke<RosettaJobBundle>("save_rosetta_segments", {
    jobId,
    segments,
  });
}

export function updateRosettaJobLanguages(
  jobId: string,
  sourceLang: string | null,
  targetLang: string
) {
  return invoke<RosettaJobBundle>("update_rosetta_job_languages", {
    jobId,
    sourceLang,
    targetLang,
  });
}

export function createRosettaTranslationRevision(
  jobId: string,
  fileId: string,
  reason: TranslationRevisionReason,
  scopeBlockIds?: string[] | null
) {
  return invoke<RosettaJobBundle>("create_rosetta_translation_revision", {
    jobId,
    fileId,
    reason,
    scopeBlockIds,
  });
}

export function renameRosettaJob(jobId: string, name: string) {
  return invoke<RosettaJobSummary[]>("rename_rosetta_job", {
    jobId,
    name,
  });
}

export function deleteRosettaJob(jobId: string) {
  return invoke<RosettaJobSummary[]>("delete_rosetta_job", { jobId });
}

export function deleteRosettaJobFile(jobId: string, fileId: string) {
  return invoke<RosettaJobFileDeleteResult>("delete_rosetta_job_file", {
    jobId,
    fileId,
  });
}

export function exportRosettaJobFile(
  jobId: string,
  fileId: string,
  kind: RosettaExportKind,
  targetPath: string
) {
  return invoke<RosettaExportResult>("export_rosetta_job_file", {
    jobId,
    fileId,
    kind,
    targetPath,
  });
}
