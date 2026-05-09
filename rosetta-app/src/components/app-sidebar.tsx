import { useEffect, useState } from "react";
import { matchPath, NavLink, useLocation } from "react-router-dom";
import {
  FileCheckIcon,
  FileClockIcon,
  FileTextIcon,
  FileXIcon,
  FolderIcon,
  PencilIcon,
  PlusIcon,
  SettingsIcon,
} from "lucide-react";

import { loadRosettaJob, renameRosettaJob } from "@/lib/rosettaJobs";
import { rosettaJobFilePath } from "@/lib/rosettaRoutes";
import { useRosettaStore } from "@/store/useRosettaStore";
import {
  Collapsible,
  CollapsibleContent,
} from "@/components/ui/collapsible";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuAction,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
  SidebarRail,
} from "@/components/ui/sidebar";
import type {
  ActiveTranslationRun,
  RosettaJobSummary,
  RosettaSourceFile,
  SourceFileTranslationStatus,
} from "@/types/rosetta";

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  const location = useLocation();
  const jobs = useRosettaStore((state) => state.jobs);
  const activeJobId = useRosettaStore((state) => state.activeJobId);
  const activeFileId = useRosettaStore((state) => state.activeFileId);
  const activeFileIdByJobId = useRosettaStore(
    (state) => state.activeFileIdByJobId
  );
  const activeDocument = useRosettaStore((state) => state.activeDocument);
  const activeTranslationRun = useRosettaStore(
    (state) => state.activeTranslationRun
  );
  const setJobList = useRosettaStore((state) => state.setJobList);
  const refreshJobBundle = useRosettaStore((state) => state.refreshJobBundle);
  const setActiveJobSelection = useRosettaStore(
    (state) => state.setActiveJobSelection
  );
  const [openJobIds, setOpenJobIds] = useState<Set<string>>(() => new Set());
  const routeJobId =
    matchPath("/jobs/:jobId/files/:fileId", location.pathname)?.params.jobId ??
    matchPath("/jobs/:jobId", location.pathname)?.params.jobId ??
    null;
  const routeFileId =
    matchPath("/jobs/:jobId/files/:fileId", location.pathname)?.params.fileId ??
    null;
  const visibleJobId = routeJobId ?? activeJobId;

  useEffect(() => {
    if (!visibleJobId) {
      return;
    }
    setOpenJobIds((current) => {
      if (current.has(visibleJobId)) {
        return current;
      }
      const next = new Set(current);
      next.add(visibleJobId);
      return next;
    });
  }, [visibleJobId]);

  function toggleJob(jobId: string) {
    setOpenJobIds((current) => {
      const next = new Set(current);
      if (next.has(jobId)) {
        next.delete(jobId);
      } else {
        next.add(jobId);
      }
      return next;
    });
  }

  async function renameJob(job: RosettaJobSummary) {
    const nextName = window.prompt("项目名", job.filename);
    if (nextName == null || nextName.trim() === job.filename) {
      return;
    }

    try {
      const nextJobs = await renameRosettaJob(job.id, nextName);
      setJobList(nextJobs);
      if (activeJobId === job.id) {
        const bundle = await loadRosettaJob(job.id);
        refreshJobBundle(bundle);
      }
    } catch (error) {
      window.alert(error instanceof Error ? error.message : "重命名项目失败。");
    }
  }

  return (
    <Sidebar collapsible="offcanvas" {...props}>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton asChild size="lg" tooltip="新项目">
              <NavLink to="/new">
                <PlusIcon />
                <span>新项目</span>
              </NavLink>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>

      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupLabel>项目</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {jobs.map((job) => {
                const files = jobFiles(job, activeJobId, activeDocument);
                const isActive = visibleJobId === job.id;
                const isOpen = openJobIds.has(job.id);
                const routeSelectedFileId =
                  routeJobId === job.id &&
                  routeFileId &&
                  files.some((file) => file.id === routeFileId)
                    ? routeFileId
                    : null;
                const selectedFileId =
                  routeSelectedFileId ??
                  activeFileIdByJobId[job.id] ??
                  (activeJobId === job.id && activeFileId
                    ? activeFileId
                    : files[0]?.id ?? null);

                return (
                  <Collapsible
                    key={job.id}
                    open={isOpen}
                  >
                    <SidebarMenuItem>
                      <SidebarMenuButton
                        aria-expanded={isOpen}
                        className="pr-8"
                        isActive={isActive}
                        onClick={() => {
                          toggleJob(job.id);
                        }}
                        tooltip={job.filename}
                        type="button"
                      >
                        {files.length > 0 ? <FolderIcon /> : <FileTextIcon />}
                        <span>{job.filename}</span>
                      </SidebarMenuButton>
                      <SidebarMenuAction
                        onClick={(event) => {
                          event.preventDefault();
                          event.stopPropagation();
                          void renameJob(job);
                        }}
                        showOnHover
                        title="重命名项目"
                        type="button"
                      >
                        <PencilIcon />
                      </SidebarMenuAction>
                      {files.length > 0 ? (
                        <CollapsibleContent>
                          <SidebarMenuSub>
                            {files.map((file) => (
                              <FileMenuItem
                                activeTranslationRun={activeTranslationRun}
                                file={file}
                                isActive={isActive && selectedFileId === file.id}
                                jobId={job.id}
                                key={`${job.id}-${file.id}`}
                                onSelect={() => {
                                  setActiveJobSelection(job.id, file.id);
                                }}
                              />
                            ))}
                          </SidebarMenuSub>
                        </CollapsibleContent>
                      ) : null}
                    </SidebarMenuItem>
                  </Collapsible>
                );
              })}

              {jobs.length === 0 && (
                <SidebarMenuItem>
                  <SidebarMenuButton className="text-muted-foreground" disabled>
                    <FileTextIcon />
                    <span>暂无项目</span>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              )}
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>

      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              asChild
              isActive={location.pathname === "/settings"}
              tooltip="设置"
            >
              <NavLink to="/settings">
                <SettingsIcon />
                <span>设置</span>
              </NavLink>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>

      <SidebarRail />
    </Sidebar>
  );
}

function FileMenuItem({
  activeTranslationRun,
  file,
  isActive,
  jobId,
  onSelect,
}: {
  activeTranslationRun: ActiveTranslationRun | null;
  file: RosettaSourceFile;
  isActive: boolean;
  jobId: string;
  onSelect: () => void;
}) {
  const status = fileTranslationStatus(jobId, file, activeTranslationRun);

  return (
    <SidebarMenuSubItem>
      <SidebarMenuSubButton asChild isActive={isActive}>
        <NavLink
          onClick={onSelect}
          to={rosettaJobFilePath(jobId, file.id)}
          title={`${file.relativePath} · ${fileStatusLabel(status)}`}
        >
          <FileStatusIcon status={status} />
          <span>{file.relativePath}</span>
        </NavLink>
      </SidebarMenuSubButton>
    </SidebarMenuSubItem>
  );
}

function fileTranslationStatus(
  jobId: string,
  file: RosettaSourceFile,
  activeTranslationRun: ActiveTranslationRun | null
): SourceFileTranslationStatus {
  if (
    activeTranslationRun?.jobId === jobId &&
    activeTranslationRun.fileId === file.id
  ) {
    return "translating";
  }
  if (file.translationStatus) {
    return file.translationStatus;
  }
  if ((file.translatingSegments ?? 0) > 0) {
    return "translating";
  }
  if ((file.failedSegments ?? 0) > 0) {
    return "failed";
  }
  const segmentCount = file.segmentCount ?? 0;
  if (segmentCount > 0 && (file.completedSegments ?? 0) >= segmentCount) {
    return "translated";
  }
  return "untranslated";
}

function FileStatusIcon({
  status,
}: {
  status: SourceFileTranslationStatus;
}) {
  const iconClassName =
    status === "translating"
      ? "rosetta-file-status-icon animate-pulse"
      : "rosetta-file-status-icon";

  if (status === "translated") {
    return (
      <span className={iconClassName} data-status={status}>
        <FileCheckIcon aria-label="已翻译" />
      </span>
    );
  }
  if (status === "translating") {
    return (
      <span className={iconClassName} data-status={status}>
        <FileClockIcon aria-label="翻译中" />
      </span>
    );
  }
  if (status === "failed") {
    return (
      <span className={iconClassName} data-status={status}>
        <FileXIcon aria-label="翻译失败" />
      </span>
    );
  }
  return (
    <span className={iconClassName} data-status={status}>
      <FileTextIcon aria-label="未翻译" />
    </span>
  );
}

function fileStatusLabel(status: SourceFileTranslationStatus) {
  if (status === "translated") {
    return "已翻译";
  }
  if (status === "translating") {
    return "翻译中";
  }
  if (status === "failed") {
    return "翻译失败";
  }
  return "未翻译";
}

function jobFiles(
  job: RosettaJobSummary,
  activeJobId: string | null,
  activeDocument: { files: RosettaSourceFile[] } | null
) {
  if (job.sourceFiles?.length > 0) {
    return job.sourceFiles;
  }
  if (activeJobId === job.id && activeDocument?.files.length) {
    return activeDocument.files;
  }
  return [];
}
