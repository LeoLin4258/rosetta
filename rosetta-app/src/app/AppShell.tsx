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
  const title = pageTitles[location.pathname] ?? "Rosetta";
  const themeMode = useRosettaStore((state) => state.themeMode);
  const [systemPrefersDark, setSystemPrefersDark] = useState(true);
  const isDark = themeMode === "system" ? systemPrefersDark : themeMode === "dark";

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
          <SidebarInset className="min-h-0  rounded-tl-lg overflow-hidden">
            <header className="flex h-14 shrink-0 items-center justify-between px-4">
              <div className="flex items-center justify-center gap-3">
                <SidebarTrigger />
                <Separator className="h-6" orientation="vertical" />
                <h1 className="text-lg font-semibold">{title}</h1>
              </div>
            </header>

            <div className="min-h-0 flex-1 overflow-auto">
              <Outlet />
            </div>
          </SidebarInset>
        </SidebarProvider>
      </div>
    </TooltipProvider>
  );
}
