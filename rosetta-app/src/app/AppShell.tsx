import { useEffect, useRef, useState } from "react";
import { Outlet, useLocation, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, type Theme } from "@tauri-apps/api/window";
import { FileText, Loader2 } from "lucide-react";
import { AppSidebar } from "@/components/app-sidebar";
import { WindowTitleBar } from "@/components/window-title-bar";
import { Separator } from "@/components/ui/separator";
import {
  SidebarInset,
  SidebarProvider,
  SidebarTrigger,
  useSidebar,
} from "@/components/ui/sidebar";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { createWelcomeDocument, listRosettaJobs, loadRosettaJob } from "@/lib/rosettaJobs";
import {
  getPdf2zhWorkerStatus,
  subscribePdf2zhWorkerStatus,
  type Pdf2zhWorkerStatus,
} from "@/lib/pdf2zhRuntime";
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
  const setManagedRuntimeStatus = useRosettaStore(
    (s) => s.setManagedRuntimeStatus
  );

  useEffect(() => {
    let unmounted = false;
    let unlisten: (() => void) | null = null;

    listen("rosetta-onboarding-completed", () => {
      // The hidden main window mounted before onboarding started installing
      // the runtime, so its store may still contain the boot-time
      // "not-installed" snapshot. Refresh immediately before the user can
      // start a translation in the newly shown workspace.
      void getManagedRwkvRuntimeStatus()
        .then(setManagedRuntimeStatus)
        .catch(() => {});

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
  }, [clearJobHistory, setManagedRuntimeStatus]);
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

/// Mirror the persistent pdf2zh worker lifecycle into the store. Fires an
/// initial fetch (covers the case where the backend emitted "ready" before
/// this listener attached) plus subscribes to live updates from
/// `rosetta-pdf2zh-worker-status` events.
function usePdf2zhWorkerStatusEvents() {
  const setPdf2zhWorkerStatus = useRosettaStore((s) => s.setPdf2zhWorkerStatus);

  useEffect(() => {
    let unmounted = false;
    let unlisten: (() => void) | null = null;

    void getPdf2zhWorkerStatus()
      .then((status) => {
        if (!unmounted) setPdf2zhWorkerStatus(status);
      })
      .catch(() => {});

    subscribePdf2zhWorkerStatus((status) => {
      setPdf2zhWorkerStatus(status);
    })
      .then((fn) => {
        if (unmounted) fn();
        else unlisten = fn;
      })
      .catch(console.error);

    return () => {
      unmounted = true;
      unlisten?.();
    };
  }, [setPdf2zhWorkerStatus]);
}

function MenuEventHandler() {
  const { toggleSidebar } = useSidebar();
  useMenuEvents(toggleSidebar);
  useOnboardingCompleted();
  usePdfRunProgressEvents();
  usePdf2zhWorkerStatusEvents();
  return null;
}

/// Format the warmup label as "[N/M label · 12s]" when granular progress is
/// available, falling back to a static string otherwise. Exported (via the
/// re-export at the bottom of this file) so the workspace topbar can render
/// the same string in its in-flight warming pill.
export function formatWarmupLabel(
  status: Pdf2zhWorkerStatus,
  elapsedSec?: number,
): string {
  const base = "PDF 引擎预热中";
  const { warmupStep, warmupTotalSteps, warmupLabel } = status;
  const detailParts: string[] = [];
  if (
    typeof warmupStep === "number" &&
    typeof warmupTotalSteps === "number" &&
    warmupLabel
  ) {
    detailParts.push(`${warmupStep}/${warmupTotalSteps} ${warmupLabel}`);
  }
  if (typeof elapsedSec === "number" && elapsedSec > 0) {
    detailParts.push(`${elapsedSec}s`);
  }
  return detailParts.length > 0 ? `${base} [${detailParts.join(" · ")}]` : base;
}

/// Tick a wall-clock counter while the worker is in `starting`. Resets when
/// the state leaves "starting" so the next cold spawn starts from 0.
export function useWarmupElapsedSeconds(state: string | undefined): number {
  const [elapsed, setElapsed] = useState(0);
  const startedAtRef = useRef<number | null>(null);
  useEffect(() => {
    if (state !== "starting") {
      startedAtRef.current = null;
      setElapsed(0);
      return;
    }
    startedAtRef.current = Date.now();
    setElapsed(0);
    const id = window.setInterval(() => {
      if (startedAtRef.current == null) return;
      setElapsed(Math.floor((Date.now() - startedAtRef.current) / 1000));
    }, 1000);
    return () => window.clearInterval(id);
  }, [state]);
  return elapsed;
}

/// Small status pill shown on the right side of the app header. Missing or
/// stale packs stay visible as a route into Settings; active translation state
/// is still owned by the workspace page. Tooltip carries the long-form message
/// + import wall time for debugging.
function Pdf2zhWorkerBadge({
  status,
  onOpenSettings,
}: {
  status: Pdf2zhWorkerStatus | null;
  onOpenSettings: () => void;
}) {
  const warmupElapsed = useWarmupElapsedSeconds(status?.state);
  if (!status || status.state === "idle") {
    return null;
  }

  let dotClass = "";
  let label = "";
  let spinning = false;
  let actionLabel: string | null = null;
  switch (status.state) {
    case "not-installed":
      dotClass = "bg-amber-500";
      label = status.message?.includes("需要更新") ? "PDF 组件需更新" : "PDF 组件未安装";
      actionLabel = "去设置";
      break;
    case "starting":
      dotClass = "bg-amber-500";
      label = formatWarmupLabel(status, warmupElapsed);
      spinning = true;
      break;
    case "ready":
      dotClass = "bg-emerald-500";
      label = "PDF 引擎已就绪";
      break;
    case "translating":
      dotClass = "bg-blue-500";
      label = "PDF 引擎工作中";
      spinning = true;
      break;
    case "failed":
      dotClass = "bg-rose-500";
      label = "PDF 引擎启动失败";
      break;
    default:
      return null;
  }

  const tooltipLines = [label];
  if (status.message) tooltipLines.push(status.message);
  if (typeof status.importMs === "number" && status.importMs > 0) {
    tooltipLines.push(`预热耗时 ${(status.importMs / 1000).toFixed(1)} s`);
  }
  const [tooltipTitle, ...tooltipDetails] = tooltipLines;

  const badgeContent = (
    <>
      {spinning ? (
        <Loader2 className={cn("size-2.5 animate-spin", dotClass.replace("bg-", "text-"))} />
      ) : (
        <span className={cn("size-1.5 rounded-full", dotClass)} />
      )}
      <span className="tabular-nums text-[11px] leading-none">{label}</span>
      {actionLabel ? (
        <span className="ml-0.5 border-l border-border/70 pl-1.5 text-[11px] leading-none text-foreground">
          {actionLabel}
        </span>
      ) : null}
    </>
  );

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        {actionLabel ? (
          <button
            type="button"
            onClick={onOpenSettings}
            className="flex h-6 items-center gap-1.5 rounded-lg border border-border/60 bg-muted/35 px-2 text-[11px] text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
            data-window-no-drag
          >
            {badgeContent}
          </button>
        ) : (
          <div
            className="flex h-6 items-center gap-1.5 rounded-lg border border-border/60 bg-muted/35 px-2 text-[11px] text-muted-foreground"
            data-window-no-drag
          >
            {badgeContent}
          </div>
        )}
      </TooltipTrigger>
      <TooltipContent
        side="bottom"
        align="end"
        className="!flex !w-80 max-w-[calc(100vw-1rem)] !flex-col !items-stretch !gap-1 text-left leading-5"
      >
        <div className="whitespace-nowrap font-medium">{tooltipTitle}</div>
        {tooltipDetails.map((line, idx) => (
          <div key={idx} className="whitespace-normal text-xs opacity-80 [overflow-wrap:anywhere]">
            {line}
          </div>
        ))}
      </TooltipContent>
    </Tooltip>
  );
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
  const location = useLocation();
  const navigate = useNavigate();
  const shouldAvoidMacTrafficLights = isMacPlatform && state === "collapsed";
  const pdf2zhWorker = useRosettaStore((s) => s.pdf2zhWorker);
  const activeDocument = useRosettaStore((s) => s.activeDocument);
  const activeSourceFileId = useRosettaStore((s) => s.activeSourceFileId);
  const activeSourceFile =
    activeDocument?.files.find((file) => file.id === activeSourceFileId) ??
    activeDocument?.files[0] ??
    null;
  const isWorkspacePdfFile =
    location.pathname === "/" && activeSourceFile?.format === "pdf";

  function openPdfSettings() {
    navigate("/settings");
    window.setTimeout(() => {
      document.getElementById("pdf2zh")?.scrollIntoView({
        block: "start",
        behavior: "smooth",
      });
    }, 0);
  }

  return (
    <header
      className={cn(
        "flex h-12 shrink-0 select-none items-center justify-between bg-background/95 px-4",
        isMacPlatform && "cursor-default"
      )}
      data-tauri-drag-region={isMacPlatform ? true : undefined}
      onMouseDown={onMouseDown}
    >
      <div
        className={cn(
          "flex min-w-0 flex-1 items-center justify-start gap-3 transition-transform duration-300 ease-out will-change-transform",
          shouldAvoidMacTrafficLights && "translate-x-20"
        )}
      >
        <SidebarTrigger />
        <Separator className="h-5" orientation="vertical" />
        <div className="flex min-w-0 items-center gap-2">
          <FileText className="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
          <h1 className="truncate text-sm font-semibold leading-none tracking-normal">{title}</h1>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {isWorkspacePdfFile ? (
          <Pdf2zhWorkerBadge status={pdf2zhWorker} onOpenSettings={openPdfSettings} />
        ) : null}
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
  const [systemPrefersDark, setSystemPrefersDark] = useState(
    () => window.matchMedia("(prefers-color-scheme: dark)").matches
  );
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
      // Onboarding may already have started the process while this hidden
      // main window still held a stale "installed" snapshot. Treat
      // "already running" as a cue to refresh, not as a terminal failure.
      .catch(() => null)
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
          !isMacPlatform && "rosetta-windows",
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
