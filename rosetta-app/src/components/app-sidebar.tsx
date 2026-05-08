import { useState } from "react";
import { NavLink, useLocation, useNavigate } from "react-router-dom";
import {
  ChevronRightIcon,
  FileTextIcon,
  FolderIcon,
  PencilIcon,
  PlusIcon,
  SettingsIcon,
} from "lucide-react";

import { loadRosettaJob, renameRosettaJob } from "@/lib/rosettaJobs";
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
import type { RosettaJobSummary, RosettaSourceFile } from "@/types/rosetta";

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  const location = useLocation();
  const navigate = useNavigate();
  const jobs = useRosettaStore((state) => state.jobs);
  const activeJobId = useRosettaStore((state) => state.activeJobId);
  const activeFileId = useRosettaStore((state) => state.activeFileId);
  const activeDocument = useRosettaStore((state) => state.activeDocument);
  const setJobList = useRosettaStore((state) => state.setJobList);
  const setActiveBundle = useRosettaStore((state) => state.setActiveBundle);
  const setActiveFileId = useRosettaStore((state) => state.setActiveFileId);
  const [openJobIds, setOpenJobIds] = useState<Set<string>>(() => new Set());

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
        setActiveBundle(bundle);
      }
    } catch (error) {
      window.alert(error instanceof Error ? error.message : "重命名项目失败。");
    }
  }

  return (
    <Sidebar collapsible="icon" {...props}>
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
                const isActive =
                  activeJobId === job.id || location.pathname === `/jobs/${job.id}`;
                const isOpen = openJobIds.has(job.id) || isActive;

                return (
                  <Collapsible key={job.id} open={isOpen}>
                    <SidebarMenuItem>
                      <SidebarMenuButton
                        isActive={isActive}
                        onClick={() => {
                          toggleJob(job.id);
                          setActiveFileId(files[0]?.id ?? null);
                          navigate(`/jobs/${job.id}`);
                        }}
                        tooltip={job.filename}
                      >
                        {files.length > 1 ? <FolderIcon /> : <FileTextIcon />}
                        <span>{job.filename}</span>
                        {files.length > 0 ? (
                          <ChevronRightIcon
                            className={
                              isOpen
                                ? "ml-auto rotate-90 transition-transform"
                                : "ml-auto transition-transform"
                            }
                          />
                        ) : null}
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
                              <SidebarMenuSubItem key={`${job.id}-${file.id}`}>
                                <SidebarMenuSubButton
                                  asChild
                                  isActive={isActive && activeFileId === file.id}
                                >
                                  <NavLink
                                    onClick={() => setActiveFileId(file.id)}
                                    to={`/jobs/${job.id}`}
                                    title={file.relativePath}
                                  >
                                    <FileTextIcon />
                                    <span>{file.relativePath}</span>
                                  </NavLink>
                                </SidebarMenuSubButton>
                              </SidebarMenuSubItem>
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
