import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { FileTextIcon, PencilIcon, PlusIcon, SettingsIcon } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import {
  importRosettaDocumentFromPath,
  loadRosettaJob,
  pickRosettaImportPath,
  renameRosettaJob,
} from "@/lib/rosettaJobs";
import { formatRelativeTime } from "@/lib/formatRelativeTime";
import { useRosettaStore } from "@/store/useRosettaStore";
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

export function AppSidebar({
  hasMacTitlebarOverlay = false,
  ...props
}: AppSidebarProps) {
  const navigate = useNavigate();
  const jobs = useRosettaStore((s) => s.jobs);
  const activeJobId = useRosettaStore((s) => s.activeJobId);
  const setJobList = useRosettaStore((s) => s.setJobList);
  const setActiveBundle = useRosettaStore((s) => s.setActiveBundle);
  const [renamingJobId, setRenamingJobId] = useState<string | null>(null);
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
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
    .slice(0, 5);

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
      setActiveBundle(bundle);
      navigate("/");
    } catch {
      // silent — user stays on current doc
    }
  }

  async function renameJob(job: RosettaJobSummary) {
    const nextName = window.prompt("文档名", job.filename);
    if (nextName == null || nextName.trim() === job.filename) return;

    setRenamingJobId(job.id);
    try {
      const nextJobs = await renameRosettaJob(job.id, nextName);
      setJobList(nextJobs);
    } catch (error) {
      window.alert(error instanceof Error ? error.message : "重命名失败。");
    } finally {
      setRenamingJobId(null);
    }
  }

  return (
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
          <SidebarGroupLabel>最近文档</SidebarGroupLabel>
          <SidebarGroupContent>
            <SidebarMenu>
              {recentJobs.map((job) => (
                <SidebarMenuItem key={job.id}>
                  <SidebarMenuButton
                    className="pr-8"
                    isActive={activeJobId === job.id}
                    onClick={() => void openJob(job)}
                    tooltip={job.filename}
                  >
                    <FileTextIcon />
                    <span className="flex-1 truncate">{job.filename}</span>
                    <span className="shrink-0 text-xs text-muted-foreground/40">
                      {formatRelativeTime(job.updatedAt)}
                    </span>
                  </SidebarMenuButton>
                  <SidebarMenuAction
                    disabled={renamingJobId === job.id}
                    onClick={(e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      void renameJob(job);
                    }}
                    showOnHover
                    title="重命名"
                    type="button"
                  >
                    <PencilIcon />
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
            <SidebarMenuButton
              asChild
              tooltip="设置"
            >
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
  );
}
