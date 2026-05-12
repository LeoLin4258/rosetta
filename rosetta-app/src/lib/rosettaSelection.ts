import type {
  RosettaDocument,
  RosettaJobSummary,
  RosettaTranslationFile,
} from "@/types/rosetta";

export function sourceSelectionKey(jobId: string, sourceFileId: string) {
  return `${jobId}:${sourceFileId}`;
}

export function resolveJobsPageSelection({
  activeDocument,
  activeJobId,
  activeSourceFileId,
  activeSourceFileIdByJobId,
  activeTranslationFileId,
  activeTranslationFileIdBySourceKey,
  jobs,
  routeJobId,
  routeSourceFileId,
  translationFiles,
}: {
  activeDocument: RosettaDocument | null;
  activeJobId: string | null;
  activeSourceFileId: string | null;
  activeSourceFileIdByJobId: Record<string, string>;
  activeTranslationFileId: string | null;
  activeTranslationFileIdBySourceKey: Record<string, string>;
  jobs: RosettaJobSummary[];
  routeJobId?: string;
  routeSourceFileId?: string;
  translationFiles: RosettaTranslationFile[];
}) {
  const currentJobId = routeJobId ?? activeJobId ?? jobs[0]?.id ?? null;
  const activeJob = jobs.find((job) => job.id === currentJobId) ?? null;
  const isCurrentBundleLoaded =
    activeJobId === currentJobId && activeDocument != null;
  const document = isCurrentBundleLoaded ? activeDocument : null;
  const sourceFiles = document?.files ?? activeJob?.sourceFiles ?? [];
  const selectedSourceFileId =
    routeSourceFileId ??
    (currentJobId ? activeSourceFileIdByJobId[currentJobId] : null) ??
    (activeJobId === currentJobId ? activeSourceFileId : null) ??
    sourceFiles[0]?.id ??
    null;
  const selectedSourceFile =
    sourceFiles.find((file) => file.id === selectedSourceFileId) ?? null;
  const selectedTranslationFileId =
    currentJobId && selectedSourceFileId
      ? activeTranslationFileIdBySourceKey[
          sourceSelectionKey(currentJobId, selectedSourceFileId)
        ] ?? activeTranslationFileId
      : activeTranslationFileId;
  const selectedTranslationFile =
    translationFiles.find(
      (file) =>
        file.id === selectedTranslationFileId &&
        file.sourceFileId === selectedSourceFileId
    ) ?? null;

  return {
    activeJob,
    currentJobId,
    document,
    isCurrentBundleLoaded,
    selectedSourceFile,
    selectedSourceFileId,
    selectedTranslationFile,
    sourceFiles,
  };
}
