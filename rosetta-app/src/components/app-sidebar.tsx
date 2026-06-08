import { useEffect, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
  FileCodeIcon,
  FileTextIcon,
  FileTypeIcon,
  PlusIcon,
  SettingsIcon,
  Trash2Icon,
} from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  deleteRosettaJob,
  importRosettaDocumentFromPath,
  loadRosettaJob,
  pickRosettaImportPath,
} from "@/lib/rosettaJobs";
import { formatRelativeTime } from "@/lib/formatRelativeTime";
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

export function AppSidebar({
  hasMacTitlebarOverlay = false,
  ...props
}: AppSidebarProps) {
  const navigate = useNavigate();
  const location = useLocation();
  const jobs = useRosettaStore((s) => s.jobs);
  const activeJobId = useRosettaStore((s) => s.activeJobId);
  const setJobList = useRosettaStore((s) => s.setJobList);
  const clearActiveJob = useRosettaStore((s) => s.clearActiveJob);
  const setActiveBundle = useRosettaStore((s) => s.setActiveBundle);
  const refreshJobBundle = useRosettaStore((s) => s.refreshJobBundle);
  const [pendingDeleteJob, setPendingDeleteJob] = useState<RosettaJobSummary | null>(null);
  const [isDraggingOver, setIsDraggingOver] = useState(false);

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

  const recentJobs = [...jobs]
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
  const isSettingsRoute = location.pathname.startsWith("/settings");

  async function addNewDocument() {
    try {
      const path = await pickRosettaImportPath();
      if (!path) return;
      const bundle = await importRosettaDocumentFromPath(path);
      setActiveBundle(bundle);
      navigate("/");
    } catch {
      // silent — picker cancel or import error
    }
  }

  async function openJob(job: RosettaJobSummary) {
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
    } catch {
      // silent — user stays on current doc
    }
  }

  async function confirmDelete() {
    if (!pendingDeleteJob) return;
    const jobId = pendingDeleteJob.id;
    setPendingDeleteJob(null);
    try {
      const nextJobs = await deleteRosettaJob(jobId);
      setJobList(nextJobs);
      if (activeJobId === jobId) clearActiveJob();
    } catch {
      // silent — job may already be gone
    }
  }

  return (
    <>
      <Sidebar collapsible="offcanvas" {...props}>
        <SidebarHeader className={cn(hasMacTitlebarOverlay && "pt-13")}>
          <SidebarMenu>
            <SidebarMenuItem>
              <SidebarMenuButton
                onClick={() => void addNewDocument()}
                tooltip="打开文件"
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
              <SidebarMenu>
                {recentJobs.map((job) => (
                  <SidebarMenuItem key={job.id}>
                    <SidebarMenuButton
                      className="gap-1 !pr-2"
                      isActive={!isSettingsRoute && activeJobId === job.id}
                      onClick={() => void openJob(job)}
                      tooltip={job.filename}
                    >
                      <DocumentFormatIcon format={job.format} />
                      <span className="min-w-0 flex-1 truncate">{job.filename}</span>
                      <span className="min-w-[2.25ch] shrink-0 text-right text-xs tabular-nums text-muted-foreground/45 transition-opacity duration-150 group-hover/menu-item:opacity-0 group-focus-within/menu-item:opacity-0 group-data-active/menu-button:text-sidebar-accent-foreground/65">
                        {formatRelativeTime(job.updatedAt)}
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
                ))}

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
