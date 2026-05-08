import { NavLink, useLocation } from "react-router-dom";
import { FileTextIcon, PlusIcon, SettingsIcon } from "lucide-react";

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
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarRail,
} from "@/components/ui/sidebar";

export function AppSidebar({ ...props }: React.ComponentProps<typeof Sidebar>) {
  const location = useLocation();
  const jobs = useRosettaStore((state) => state.jobs);
  const activeJobId = useRosettaStore((state) => state.activeJobId);

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
              {jobs.map((job) => (
                <SidebarMenuItem key={job.id}>
                  <SidebarMenuButton
                    asChild
                    isActive={
                      activeJobId === job.id ||
                      location.pathname === `/jobs/${job.id}`
                    }
                    tooltip={job.filename}
                  >
                    <NavLink to={`/jobs/${job.id}`}>
                      <FileTextIcon />
                      <span>{job.filename}</span>
                    </NavLink>
                  </SidebarMenuButton>
                </SidebarMenuItem>
              ))}

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
