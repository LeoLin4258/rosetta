import { useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  AlertCircleIcon,
  CheckCircle2Icon,
  CircleIcon,
  FileCodeIcon,
  FileTextIcon,
  FileTypeIcon,
  Loader2Icon,
  PlusIcon,
  SettingsIcon,
  Trash2Icon,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  deleteRosettaJob,
  loadRosettaJob,
  repairRosettaPdfJob,
} from "@/lib/rosettaJobs";
import { useRosettaStore } from "@/store/useRosettaStore";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
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
  SidebarRail,
} from "@/components/ui/sidebar";
import type { RosettaJobSummary } from "@/types/rosetta";
import { cn } from "@/lib/utils";

type AppSidebarProps = React.ComponentProps<typeof Sidebar> & {
  hasMacTitlebarOverlay?: boolean;
};

function DocumentFormatIcon({ format }: { format: RosettaJobSummary["format"] }) {
  if (format === "pdf") return <FileTypeIcon />;
  if (format === "markdown") return <FileCodeIcon />;
  return <FileTextIcon />;
}

type SidebarJobTranslationState =
  | "running"
  | "completed"
  | "untranslated"
  | "failed"
  | "partial";

function getSidebarJobTranslationState(
  job: RosettaJobSummary,
  isRunning: boolean
): SidebarJobTranslationState {
  if (isRunning) return "running";
  if (job.failedSegments > 0 || job.status === "failed") return "failed";
  if (
    job.status === "completed" ||
    (job.segmentCount > 0 && job.completedSegments >= job.segmentCount)
  ) {
    return "completed";
  }
  if (job.completedSegments > 0) return "partial";
  return "untranslated";
}

function sidebarJobStatusLabel(
  state: SidebarJobTranslationState,
  completed: number,
  total: number
) {
  if (state === "running") {
    return total > 0 ? `正在翻译，${completed}/${total}` : "正在翻译";
  }
  if (state === "completed") return "已完成";
  if (state === "failed") return "翻译失败";
  if (state === "partial") return total > 0 ? `已翻译 ${completed}/${total}` : "部分已翻译";
  return "未翻译";
}

function SidebarJobStatusIndicator({
  state,
  completed,
  total,
}: {
  state: SidebarJobTranslationState;
  completed: number;
  total: number;
}) {
  const label = sidebarJobStatusLabel(state, completed, total);

  if (state === "running") {
    return (
      <span
        className="inline-flex size-5 shrink-0 items-center justify-center rounded-md bg-blue-500/10 text-blue-700 ring-1 ring-blue-500/25 dark:text-blue-300"
        aria-label={label}
        role="img"
        title={label}
      >
        <Loader2Icon className="size-3 animate-spin motion-reduce:animate-none" />
      </span>
    );
  }

  if (state === "completed") {
    return (
      <span
        className="inline-flex size-5 shrink-0 items-center justify-center rounded-md text-emerald-700 dark:text-emerald-300"
        aria-label={label}
        role="img"
        title={label}
      >
        <CheckCircle2Icon className="size-4" />
      </span>
    );
  }

  if (state === "failed") {
    return (
      <span
        className="inline-flex size-5 shrink-0 items-center justify-center rounded-md text-rose-700 dark:text-rose-300"
        aria-label={label}
        role="img"
        title={label}
      >
        <AlertCircleIcon className="size-4" />
      </span>
    );
  }

  if (state === "partial") {
    return (
      <span
        className="inline-flex size-5 shrink-0 items-center justify-center rounded-md text-amber-700 dark:text-amber-300"
        aria-label={label}
        role="img"
        title={label}
      >
        <CircleIcon className="size-3.5 fill-current" />
      </span>
    );
  }

  return (
    <span
      className="inline-flex size-5 shrink-0 items-center justify-center rounded-md text-sidebar-foreground/45"
      aria-label={label}
      role="img"
      title={label}
    >
      <CircleIcon className="size-3.5" />
    </span>
  );
}

export function AppSidebar({
  hasMacTitlebarOverlay = false,
  ...props
}: AppSidebarProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const jobs = useRosettaStore((s) => s.jobs);
  const activeJobId = useRosettaStore((s) => s.activeJobId);
  const activeTranslationRun = useRosettaStore((s) => s.activeTranslationRun);
  const setJobList = useRosettaStore((s) => s.setJobList);
  const clearActiveJob = useRosettaStore((s) => s.clearActiveJob);
  const setActiveBundle = useRosettaStore((s) => s.setActiveBundle);
  const refreshJobBundle = useRosettaStore((s) => s.refreshJobBundle);
  const [pendingDeleteJob, setPendingDeleteJob] = useState<RosettaJobSummary | null>(null);
  const [isDraggingOver, setIsDraggingOver] = useState(false);
  const [sidebarError, setSidebarError] = useState<string | null>(null);

  // Lightweight drag listener for visual feedback only — does not handle drop.
  // Same StrictMode-safe async cleanup pattern as WorkspacePage.
  useEffect(() => {
    const appWindow = getCurrentWindow();
    let unmounted = false;
    let unlisten: (() => void) | null = null;

    appWindow
      .onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setIsDraggingOver(true);
        } else if (
          event.payload.type === "leave" ||
          event.payload.type === "drop"
        ) {
          setIsDraggingOver(false);
        }
      })
      .then((fn) => {
        if (unmounted) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(console.error);

    return () => {
      unmounted = true;
      unlisten?.();
    };
  }, []);

  const recentJobs = [...jobs].sort(
    (a, b) => b.createdAt.localeCompare(a.createdAt) || b.id.localeCompare(a.id)
  );
  const isSettingsRoute = location.pathname.startsWith("/settings");

  function addNewDocument() {
    clearActiveJob();
    navigate("/");
  }

  async function openJob(job: RosettaJobSummary) {
    setSidebarError(null);
    try {
      const bundle = await loadRosettaJob(job.id);
      // Same job: preserve existing translationSegments (don't clear them).
      // Different job: setActiveBundle clears all stale in-memory state.
      if (activeJobId === job.id) {
        refreshJobBundle(bundle);
      } else {
        setActiveBundle(bundle);
      }
      navigate("/");
    } catch (error) {
      if (job.format === "pdf") {
        try {
          const repair = await repairRosettaPdfJob(job.id);
          if (!repair.recoverable) {
            setSidebarError(
              repair.warnings[0] ?? "文档数据损坏，可删除或重新导入。",
            );
            return;
          }
          if (repair.recoverable) {
            const bundle = await loadRosettaJob(job.id);
            if (activeJobId === job.id) {
              refreshJobBundle(bundle);
            } else {
              setActiveBundle(bundle);
            }
            navigate("/");
            return;
          }
        } catch (repairError) {
          setSidebarError(errorMessage(repairError, "文档数据损坏，可删除或重新导入。"));
          return;
        }
      }
      setSidebarError(errorMessage(error, "无法打开文档。"));
    }
  }

  async function confirmDelete() {
    if (!pendingDeleteJob) return;
    const jobId = pendingDeleteJob.id;
    setPendingDeleteJob(null);
    setSidebarError(null);
    try {
      const result = await deleteRosettaJob(jobId);
      setJobList(result.jobs);
      if (activeJobId === jobId) clearActiveJob();
      if (result.warning) {
        setSidebarError(result.warning);
      }
    } catch (error) {
      setSidebarError(errorMessage(error, "无法删除文档。"));
    }
  }

  return (
    <>
      <Sidebar collapsible="offcanvas" {...props}>
        <SidebarHeader className={cn(hasMacTitlebarOverlay && "pt-13")}>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton
                onClick={addNewDocument}
                isActive={location.pathname === "/" && !activeJobId}
                tooltip="新建文档"
              >
                <PlusIcon />
                <span>新建文档</span>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarHeader>

        <SidebarContent className="relative">
          {isDraggingOver && (
            <div className="pointer-events-none absolute inset-2 z-10 flex flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-primary/60 bg-primary/5">
              <PlusIcon className="size-6 text-primary/70" strokeWidth={1.5} />
              <span className="text-xs font-medium text-primary/70">拖入以添加文档</span>
            </div>
          )}
          <SidebarGroup>
            <SidebarGroupLabel>文档</SidebarGroupLabel>
            <SidebarGroupContent>
              {sidebarError ? (
                <div className="mx-2 mb-2 rounded-md border border-destructive/30 bg-destructive/5 px-2 py-1.5 text-xs leading-5 text-destructive">
                  {sidebarError}
                </div>
              ) : null}
              <SidebarMenu>
                {recentJobs.map((job) => {
                  const isRunning = activeTranslationRun?.jobId === job.id;
                  const translationState = getSidebarJobTranslationState(job, isRunning);
                  const completed = isRunning
                    ? activeTranslationRun.completedSegmentIds.length
                    : job.completedSegments;
                  const total = isRunning
                    ? activeTranslationRun.targetSegmentIds.length
                    : job.segmentCount;
                  const progressPercent =
                    total > 0 ? Math.min(100, Math.round((completed / total) * 100)) : 0;
                  const isActive = !isSettingsRoute && activeJobId === job.id;
                  const emphasizeRunning = translationState === "running" && isActive;

                  return (
                    <SidebarMenuItem key={job.id}>
                      <SidebarMenuButton
                        className={cn(
                          "relative h-9 gap-1.5 !pr-2 text-[0.8125rem]",
                          emphasizeRunning &&
                            "bg-blue-500/10 text-sidebar-foreground ring-1 ring-blue-500/25 hover:bg-blue-500/15 data-active:bg-blue-500/15 data-active:text-sidebar-foreground"
                        )}
                        isActive={isActive}
                        onClick={() => void openJob(job)}
                        tooltip={job.filename}
                      >
                        <DocumentFormatIcon format={job.format} />
                        <span className="min-w-0 flex-1 truncate">{job.filename}</span>
                        <span className="flex shrink-0 items-center transition-opacity duration-150 group-hover/menu-item:opacity-0 group-focus-within/menu-item:opacity-0">
                          <SidebarJobStatusIndicator
                            state={translationState}
                            completed={completed}
                            total={total}
                          />
                          {translationState === "running" && (
                            <span
                              className="pointer-events-none absolute right-2 bottom-1 left-7 h-0.5 overflow-hidden rounded-full bg-blue-500/15"
                              aria-hidden="true"
                            >
                              <span
                                className="block h-full rounded-full bg-blue-500/70 transition-[width] duration-300 ease-out motion-reduce:transition-none"
                                style={{ width: `${progressPercent}%` }}
                              />
                            </span>
                          )}
                        </span>
                      </SidebarMenuButton>
                      <SidebarMenuAction
                        className="right-1.5 bg-sidebar/90 text-sidebar-foreground/65 shadow-sm backdrop-blur hover:bg-sidebar-accent hover:text-sidebar-accent-foreground"
                        onClick={(e) => {
                          e.preventDefault();
                          e.stopPropagation();
                          setPendingDeleteJob(job);
                        }}
                        showOnHover
                        title="删除"
                        type="button"
                      >
                        <Trash2Icon />
                      </SidebarMenuAction>
                    </SidebarMenuItem>
                  );
                })}

                {recentJobs.length === 0 && (
                  <SidebarMenuItem>
                    <SidebarMenuButton className="text-muted-foreground/50" disabled>
                      <FileTextIcon />
                      <span>暂无文档</span>
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
              <SidebarMenuButton asChild isActive={isSettingsRoute} tooltip="设置">
                <button type="button" onClick={() => navigate("/settings")}>
                  <SettingsIcon />
                  <span>设置</span>
                </button>
              </SidebarMenuButton>
            </SidebarMenuItem>
          </SidebarMenu>
        </SidebarFooter>

        <SidebarRail />
      </Sidebar>

      <AlertDialog
        open={!!pendingDeleteJob}
        onOpenChange={(open) => { if (!open) setPendingDeleteJob(null); }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>删除文档</AlertDialogTitle>
            <AlertDialogDescription>
              确定要删除「{pendingDeleteJob?.filename}」吗？此操作将移除翻译记录，无法撤销。
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>取消</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => void confirmDelete()}
              className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
            >
              删除
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

function errorMessage(error: unknown, fallback: string) {
  if (error instanceof Error && error.message.trim()) return error.message;
  if (typeof error === "string" && error.trim()) return error;
  return fallback;
}
