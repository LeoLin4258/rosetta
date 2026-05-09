import type { RosettaJobSummary } from "@/types/rosetta";

export function rosettaJobPath(jobId: string) {
  return `/jobs/${encodeURIComponent(jobId)}`;
}

export function rosettaJobFilePath(jobId: string, fileId: string) {
  return `${rosettaJobPath(jobId)}/files/${encodeURIComponent(fileId)}`;
}

export function rosettaJobDefaultPath(job: RosettaJobSummary) {
  const firstFileId = job.sourceFiles[0]?.id;
  return firstFileId ? rosettaJobFilePath(job.id, firstFileId) : rosettaJobPath(job.id);
}
