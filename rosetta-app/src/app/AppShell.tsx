import { useEffect, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { getCurrentWindow, type Theme } from "@tauri-apps/api/window";
import { AppSidebar } from "@/components/app-sidebar";
import { WindowTitleBar } from "@/components/window-title-bar";
import { Separator } from "@/components/ui/separator";
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { TooltipProvider } from "@/components/ui/tooltip";
import { listRosettaJobs } from "@/lib/rosettaJobs";
import { useRosettaStore } from "@/store/useRosettaStore";
import { cn } from "@/lib/utils";

const pageTitles: Record<string, string> = {
  "/": "Rosetta",
  "/new": "新项目",
  "/jobs": "任务",
  "/settings": "设置",
};

const appWindow = getCurrentWindow();

export function AppShell() {
  const location = useLocation();
  const themeMode = useRosettaStore((state) => state.themeMode);
  const setJobList = useRosettaStore((state) => state.setJobList);
  const jobs = useRosettaStore((state) => state.jobs);
  const activeJobId = useRosettaStore((state) => state.activeJobId);
  const [systemPrefersDark, setSystemPrefersDark] = useState(true);
  const isDark = themeMode === "system" ? systemPrefersDark : themeMode === "dark";
  const routeJobId = location.pathname.match(/^\/jobs\/([^/]+)/)?.[1] ?? null;
  const currentJobId = routeJobId ?? activeJobId;
  const currentJob = jobs.find((job) => job.id === currentJobId) ?? null;
  const title =
    location.pathname === "/jobs" || location.pathname.startsWith("/jobs/")
      ? currentJob?.filename ?? "任务"
      : pageTitles[location.pathname] ?? "Rosetta";

  useEffect(() => {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    function syncSystemTheme() {
      setSystemPrefersDark(mediaQuery.matches);
    }

    syncSystemTheme();
    mediaQuery.addEventListener("change", syncSystemTheme);

    return () => mediaQuery.removeEventListener("change", syncSystemTheme);
  }, []);

  useEffect(() => {
    const windowTheme: Theme | null = themeMode === "system" ? null : themeMode;

    void appWindow.setTheme(windowTheme).catch(() => {
      // Plain browser dev mode does not expose the Tauri window API.
    });
  }, [themeMode]);

  useEffect(() => {
    void listRosettaJobs()
      .then(setJobList)
      .catch(() => {
        setJobList([]);
      });
  }, [setJobList]);

  return (
    <TooltipProvider>
      <div
        className={cn(
          "flex h-screen flex-col bg-transparent text-foreground",
          isDark && "dark"
        )}
      >
        <WindowTitleBar />
        <SidebarProvider
          className="h-full min-h-0 flex-1 bg-transparent text-foreground"
          style={
            {
              "--window-titlebar-height": "2.25rem",
            } as React.CSSProperties
          }
        >
          <AppSidebar />
          <SidebarInset className="min-h-0  rounded-tl-xl overflow-hidden border-l border-t">
            <header className="flex h-14 shrink-0 items-center justify-between px-4">
              <div className="flex items-center justify-center gap-3">
                <SidebarTrigger />
                <Separator className="h-6" orientation="vertical" />
                <h1 className="text-lg font-semibold">{title}</h1>
              </div>
            </header>

            <div className="min-h-0 flex-1 ">
                <Outlet />
            </div>
          </SidebarInset>
        </SidebarProvider>
      </div>
    </TooltipProvider>
  );
}
