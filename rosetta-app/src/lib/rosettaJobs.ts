import { invoke } from "@tauri-apps/api/core";
import type {
  RosettaExportKind,
  RosettaExportResult,
  RosettaJobBundle,
  RosettaJobSummary,
  Segment,
} from "../types/rosetta";

export function importRosettaDocumentFromPath(path: string) {
  return invoke<RosettaJobBundle>("import_rosetta_document_from_path", { path });
}

export function pickRosettaImportPath() {
  return invoke<string | null>("pick_rosetta_import_path");
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

export function deleteRosettaJob(jobId: string) {
  return invoke<RosettaJobSummary[]>("delete_rosetta_job", { jobId });
}

export function exportRosettaJob(
  jobId: string,
  kind: RosettaExportKind,
  targetPath: string
) {
  return invoke<RosettaExportResult>("export_rosetta_job", {
    jobId,
    kind,
    targetPath,
  });
}
