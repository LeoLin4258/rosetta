import { useState } from "react";
import { matchPath, NavLink, useLocation } from "react-router-dom";
import { FolderIcon, PencilIcon, PlusIcon, SettingsIcon } from "lucide-react";

import { loadRosettaJob, renameRosettaJob } from "@/lib/rosettaJobs";
import { rosettaJobDefaultPath } from "@/lib/rosettaRoutes";
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

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  const location = useLocation();
  const jobs = useRosettaStore((state) => state.jobs);
  const activeJobId = useRosettaStore((state) => state.activeJobId);
  const setJobList = useRosettaStore((state) => state.setJobList);
  const refreshJobBundle = useRosettaStore((state) => state.refreshJobBundle);
  const [renamingJobId, setRenamingJobId] = useState<string | null>(null);
  const routeJobId =
    matchPath("/jobs/:jobId/files/:fileId", location.pathname)?.params.jobId ??
    matchPath("/jobs/:jobId", location.pathname)?.params.jobId ??
    null;
  const visibleJobId = routeJobId ?? activeJobId;

  async function renameJob(job: RosettaJobSummary) {
    const nextName = window.prompt("项目名", job.filename);
    if (nextName == null || nextName.trim() === job.filename) {
      return;
    }

    setRenamingJobId(job.id);
    try {
      const nextJobs = await renameRosettaJob(job.id, nextName);
      setJobList(nextJobs);
      if (activeJobId === job.id) {
        refreshJobBundle(await loadRosettaJob(job.id));
      }
    } catch (error) {
      window.alert(error instanceof Error ? error.message : "重命名项目失败。");
    } finally {
      setRenamingJobId(null);
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
              {jobs.map((job) => (
                <SidebarMenuItem key={job.id}>
                  <SidebarMenuButton
                    asChild
                    className="pr-8"
                    isActive={visibleJobId === job.id}
                    tooltip={job.filename}
                  >
                    <NavLink to={rosettaJobDefaultPath(job)}>
                      <FolderIcon />
                      <span>{job.filename}</span>
                    </NavLink>
                  </SidebarMenuButton>
                  <SidebarMenuAction
                    disabled={renamingJobId === job.id}
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
                </SidebarMenuItem>
              ))}

              {jobs.length === 0 && (
                <SidebarMenuItem>
                  <SidebarMenuButton className="text-muted-foreground" disabled>
                    <FolderIcon />
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
