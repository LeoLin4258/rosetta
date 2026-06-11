import { useEffect, useRef, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, type Theme } from "@tauri-apps/api/window";
import { AppSidebar } from "@/components/app-sidebar";
import { WindowTitleBar } from "@/components/window-title-bar";
import { Separator } from "@/components/ui/separator";
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
  useSidebar,
} from "@/components/ui/sidebar";
import { TooltipProvider } from "@/components/ui/tooltip";
import { createWelcomeDocument, listRosettaJobs, loadRosettaJob } from "@/lib/rosettaJobs";
import { getManagedRwkvRuntimeStatus, startManagedRwkvRuntime } from "@/lib/rwkvRuntime";
import { useMenuEvents } from "@/lib/useMenuEvents";
import { useRosettaStore } from "@/store/useRosettaStore";
import { cn } from "@/lib/utils";

const isMacPlatform =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad|iPod/.test(navigator.platform);

const pageTitles: Record<string, string> = {
  "/settings": "设置",
};

const appWindow = getCurrentWindow();

/// Load the job list and, when the workspace is empty, create + activate the
/// welcome document. Shared by first mount and the post-onboarding reset so
/// both paths end in the same deterministic state.
async function bootstrapJobList() {
  const { setJobList, setActiveBundle } = useRosettaStore.getState();
  try {
    const jobs = await listRosettaJobs();
    setJobList(jobs);
    if (jobs.length === 0) {
      const bundle = await createWelcomeDocument();
      setJobList([bundle.job]);
      setActiveBundle(bundle);
    }
  } catch {
    setJobList([]);
  }
}

function useOnboardingCompleted() {
  const clearJobHistory = useRosettaStore((s) => s.clearJobHistory);

  useEffect(() => {
    let unmounted = false;
    let unlisten: (() => void) | null = null;

    listen("rosetta-onboarding-completed", () => {
      // The main window may have bootstrapped its job list while onboarding
      // was still open (both windows exist from app start). Re-bootstrap
      // after clearing so the welcome document reliably shows instead of
      // depending on event timing.
      clearJobHistory();
      void bootstrapJobList();
    }).then((fn) => {
      if (unmounted) { fn(); } else { unlisten = fn; }
    }).catch(console.error);

    return () => { unmounted = true; unlisten?.(); };
  }, [clearJobHistory]);
}

/// App-level subscription to pdf2zh progress events. Lives here (not in
/// WorkspacePage) and writes to the store keyed by jobId, so switching files
/// or navigating to Settings during a long PDF run doesn't lose the live
/// phase/page display.
function usePdfRunProgressEvents() {
  const setPdfRunProgress = useRosettaStore((s) => s.setPdfRunProgress);

  useEffect(() => {
    let unmounted = false;
    let unlisten: (() => void) | null = null;

    listen<{
      jobId: string;
      phase: string;
      percent: number | null;
      currentPage?: number;
      totalPages?: number;
      translatedChars?: number;
    }>("rosetta-pdf2zh-progress", (event) => {
      setPdfRunProgress(event.payload.jobId, {
        phase: event.payload.phase,
        percent: event.payload.percent,
        currentPage: event.payload.currentPage ?? null,
        totalPages: event.payload.totalPages ?? null,
        translatedChars: event.payload.translatedChars ?? null,
      });
    }).then((fn) => {
      if (unmounted) fn();
      else unlisten = fn;
    }).catch(console.error);

    return () => { unmounted = true; unlisten?.(); };
  }, [setPdfRunProgress]);
}

function MenuEventHandler() {
  const { toggleSidebar } = useSidebar();
  useMenuEvents(toggleSidebar);
  useOnboardingCompleted();
  usePdfRunProgressEvents();
  return null;
}

function AppHeader({
  isMacPlatform,
  onMouseDown,
  title,
}: {
  isMacPlatform: boolean;
  onMouseDown: (event: React.MouseEvent<HTMLElement>) => void;
  title: string;
}) {
  const { state } = useSidebar();
  const shouldAvoidMacTrafficLights = isMacPlatform && state === "collapsed";

  return (
    <header
      className={cn(
        "flex h-14 shrink-0 select-none items-center justify-between px-4",
        isMacPlatform && "cursor-default"
      )}
      data-tauri-drag-region={isMacPlatform ? true : undefined}
      onMouseDown={onMouseDown}
    >
      <div
        className={cn(
          "flex items-center justify-center gap-3 transition-transform duration-300 ease-out will-change-transform",
          shouldAvoidMacTrafficLights && "translate-x-20"
        )}
      >
        <SidebarTrigger />
        <Separator className="h-6" orientation="vertical" />
        <h1 className="text-lg font-semibold">{title}</h1>
      </div>
    </header>
  );
}

export function AppShell() {
  const location = useLocation();
  const themeMode = useRosettaStore((state) => state.themeMode);
  const activeDocument = useRosettaStore((state) => state.activeDocument);
  const activeJobId = useRosettaStore((state) => state.activeJobId);
  const managedRuntimeStatus = useRosettaStore((state) => state.managedRuntime.status);
  const setManagedRuntimeStatus = useRosettaStore((state) => state.setManagedRuntimeStatus);
  const refreshJobBundle = useRosettaStore((state) => state.refreshJobBundle);
  // Tracks whether the one-shot auto-start has been attempted this session.
  // Prevents re-starting the runtime when the user explicitly stops it.
  const runtimeAutoStartedRef = useRef(false);
  const [systemPrefersDark, setSystemPrefersDark] = useState(true);
  const isDark = themeMode === "system" ? systemPrefersDark : themeMode === "dark";
  const title = pageTitles[location.pathname] ?? activeDocument?.filename ?? "Rosetta";
  const titlebarHeight = isMacPlatform ? "0px" : "2.25rem";

  async function startHeaderDrag(event: React.MouseEvent<HTMLElement>) {
    if (!isMacPlatform || event.button !== 0) {
      return;
    }

    const target = event.target as HTMLElement;
    if (
      target.closest(
        "button, a, input, select, textarea, [role='button'], [data-window-no-drag]"
      )
    ) {
      return;
    }

    if (event.detail === 2) {
      await appWindow.toggleMaximize();
      return;
    }

    await appWindow.startDragging();
  }

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
    document.documentElement.classList.toggle("dark", isDark);
  }, [isDark]);

  useEffect(() => {
    void bootstrapJobList();
  }, []);

  // Auto-restore the active document after restart (activeJobId is persisted
  // but activeDocument is in-memory only).
  useEffect(() => {
    if (!activeJobId || activeDocument) return;
    void loadRosettaJob(activeJobId)
      .then((bundle) => refreshJobBundle(bundle))
      .catch(() => {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeJobId, activeDocument?.id]);

  // Probe managed runtime status on startup so WorkspacePage can use it.
  useEffect(() => {
    void getManagedRwkvRuntimeStatus()
      .then(setManagedRuntimeStatus)
      .catch(() => {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // When a document is open and the runtime is installed/stopped, auto-start
  // it proactively so the model is ready by the time the user clicks Translate.
  // The ref guard ensures this fires at most once per session — if the user
  // explicitly stops the runtime via Settings, it won't be restarted.
  useEffect(() => {
    if (runtimeAutoStartedRef.current) return;
    if (!activeDocument) return;
    const state = managedRuntimeStatus?.state;
    if (state !== "installed" && state !== "stopped") return;

    runtimeAutoStartedRef.current = true;
    void startManagedRwkvRuntime()
      .then(() => getManagedRwkvRuntimeStatus())
      .then(setManagedRuntimeStatus)
      .catch(() => {});
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeDocument?.id, managedRuntimeStatus?.state]);

  // Poll every 1.5 s while the runtime is starting until it reaches a terminal state.
  useEffect(() => {
    if (managedRuntimeStatus?.state !== "starting") return;

    const id = setInterval(() => {
      void getManagedRwkvRuntimeStatus().then((s) => {
        setManagedRuntimeStatus(s);
        if (s.state !== "starting") clearInterval(id);
      }).catch(() => clearInterval(id));
    }, 1500);

    return () => clearInterval(id);
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [managedRuntimeStatus?.state]);

  return (
    <TooltipProvider>
      <div
        className={cn(
          "flex h-screen flex-col bg-transparent text-foreground",
          isMacPlatform && "rosetta-macos",
          isDark && "dark"
        )}
      >
        {!isMacPlatform && <WindowTitleBar />}
        <SidebarProvider
          className="h-full min-h-0 flex-1 bg-transparent text-foreground"
          style={
            {
              "--window-titlebar-height": titlebarHeight,
            } as React.CSSProperties
          }
        >
          <MenuEventHandler />
          <AppSidebar hasMacTitlebarOverlay={isMacPlatform} />
          <SidebarInset
            className={cn(
              "min-h-0 overflow-hidden ",
              isMacPlatform ? "rounded-none" : "rounded-tl-xl border-t"
            )}
          >
            <AppHeader
              isMacPlatform={isMacPlatform}
              onMouseDown={startHeaderDrag}
              title={title}
            />

            <div className="min-h-0 flex-1 ">
                <Outlet />
            </div>
          </SidebarInset>
        </SidebarProvider>
      </div>
    </TooltipProvider>
  );
}
