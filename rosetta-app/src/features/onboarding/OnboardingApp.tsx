import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { completeOnboardingAndOpenMain, getOnboardingDecision } from "@/lib/onboarding";
import {
  isPdf2zhReady,
  useManagedPdf2zhRuntime,
} from "@/lib/useManagedPdf2zhRuntime";
import { prewarmPdf2zhWorker } from "@/lib/pdf2zhRuntime";
import { useManagedRwkvRuntime } from "@/lib/useManagedRwkvRuntime";
import { cn } from "@/lib/utils";

import { DoneStep } from "./DoneStep";
import { InstallStep } from "./InstallStep";
import { WelcomeStep } from "./WelcomeStep";

type OnboardingStep =
  | "welcome"
  | "installing-runtime"
  | "installing-pdf"
  | "done";

const appWindow = getCurrentWindow();

function formatDownloadCaption(modelSizeBytes: number | null): string {
  if (modelSizeBytes == null || modelSizeBytes <= 0)
    return "下载完成后无需再联网";
  const mb = modelSizeBytes / (1024 * 1024);
  const label =
    mb >= 1024
      ? `约 ${(mb / 1024).toFixed(1)} GB`
      : `约 ${Math.round(mb)} MB`;
  return `${label} · 下载完成后无需再联网`;
}

/**
 * Root of the onboarding window. Pure orchestration — each step component
 * stays focused on its visuals while this picks which one to show + wires
 * the "next" transitions.
 *
 * State machine:
 *   welcome --(click 安装)--> installing
 *   installing --(success)--> done
 *   installing --(failure)--> installing (with errorMessage)
 *   installing --(cancel)--> welcome
 *   done --(click continue)--> close onboarding + open main
 *   welcome --(skip link)--> close onboarding + open main (skippedLocal=true)
 *
 * If the user closes the window mid-install, `mark_onboarding_completed` was
 * never called → next launch re-opens onboarding. Existing model `.part` file
 * gives resume support via Phase 4's install logic.
 */
export function OnboardingApp() {
  const runtime = useManagedRwkvRuntime();
  const pdfRuntime = useManagedPdf2zhRuntime();
  const [systemPrefersDark, setSystemPrefersDark] = useState(
    () => window.matchMedia("(prefers-color-scheme: dark)").matches
  );

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const sync = () => setSystemPrefersDark(mq.matches);
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  }, []);

  const [step, setStep] = useState<OnboardingStep>("welcome");
  const [doneVariant, setDoneVariant] =
    useState<"local" | "local-pdf-skipped" | "external">("local");
  const [usingExternalApi, setUsingExternalApi] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [isFinishing, setIsFinishing] = useState(false);
  // Pull the decision once to feed Welcome step the model size + "are we
  // upgrading" flag. `null` while loading and on errors — Welcome falls
  // back to neutral copy in that case rather than blocking the screen.
  const [decision, setDecision] = useState<{
    modelSizeBytes: number | null;
    localInstallSizeBytes: number | null;
    isReturningUser: boolean;
  } | null>(null);
  useEffect(() => {
    let mounted = true;
    void getOnboardingDecision()
      .then((d) => {
        if (!mounted) return;
        setDecision({
          modelSizeBytes: d.modelSizeBytes,
          localInstallSizeBytes: d.localInstallSizeBytes,
          isReturningUser: d.isReturningUser,
        });
      })
      .catch(() => {
        // Best-effort: leave decision null and let Welcome step show defaults.
      });
    return () => {
      mounted = false;
    };
  }, []);

  const beginPdfSetup = useCallback(async (externalApi: boolean) => {
    setErrorMessage(null);
    setUsingExternalApi(externalApi);
    setStep("installing-pdf");
    try {
      const existing = await pdfRuntime.refreshStatus();
      if (!isPdf2zhReady(existing)) {
        await pdfRuntime.install({ repair: false });
      }
      const warmed = await prewarmPdf2zhWorker();
      if (!warmed) {
        throw new Error("PDF 组件已安装，但预热没有完成。请重试。");
      }
      setDoneVariant(externalApi ? "external" : "local");
      setStep("done");
    } catch (error) {
      setErrorMessage(toMessage(error));
    }
  }, [pdfRuntime]);

  const installLocalRuntime = useCallback(async () => {
    setErrorMessage(null);
    setStep("installing-runtime");
    const installed = await runtime.install({ repair: false });
    if (!installed) {
      setErrorMessage(runtime.lastError ?? "本地翻译引擎安装没有完成。");
      return;
    }
    const started = await runtime.start();
    if (!started) {
      setErrorMessage(runtime.lastError ?? "本地翻译引擎启动没有完成。");
      return;
    }
    const probe = await runtime.probe();
    if (!probe?.ok) {
      setErrorMessage(probe?.message ?? "本地翻译引擎探活没有完成。");
      return;
    }
    await beginPdfSetup(false);
  }, [beginPdfSetup, runtime]);

  const handleBeginInstall = useCallback(() => {
    void installLocalRuntime();
  }, [installLocalRuntime]);

  const handleRetry = useCallback(() => {
    setErrorMessage(null);
    if (step === "installing-pdf") {
      void beginPdfSetup(usingExternalApi);
    } else {
      void installLocalRuntime();
    }
  }, [beginPdfSetup, installLocalRuntime, step, usingExternalApi]);

  const handleCancel = useCallback(() => {
    if (step === "installing-pdf") {
      void pdfRuntime.cancelInstall();
    } else {
      void runtime.cancelInstall();
    }
    setStep("welcome");
    setErrorMessage(null);
  }, [pdfRuntime, runtime, step]);

  const handleSkipToExternal = useCallback(() => {
    void beginPdfSetup(true);
  }, [beginPdfSetup]);

  const handleSkipPdf = useCallback(() => {
    setDoneVariant(usingExternalApi ? "external" : "local-pdf-skipped");
    setStep("done");
    setErrorMessage(null);
  }, [usingExternalApi]);

  const handleEnterWorkspace = useCallback(async () => {
    setIsFinishing(true);
    try {
      await completeOnboardingAndOpenMain({
        skippedLocalInstall: usingExternalApi,
      });
    } catch (error) {
      console.error("complete onboarding failed", error);
      setIsFinishing(false);
    }
  }, [usingExternalApi]);

  const handleDragStripMouseDown = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      if (event.button !== 0) {
        return;
      }

      event.preventDefault();
      void appWindow.startDragging();
    },
    []
  );

  return (
    <div
      className={cn(
        "rosetta-onboarding flex h-screen flex-col select-none bg-transparent text-foreground",
        systemPrefersDark && "dark"
      )}
    >
      {/* Dedicated drag strip — sits over the macOS traffic-lights row.
          Pure empty space; no content so nothing intercepts drag events. */}
      <div
        className="h-10 w-full shrink-0"
        data-tauri-drag-region
        onMouseDown={handleDragStripMouseDown}
      />
      <div className="min-h-0 flex-1">
        {step === "welcome" && (
          <WelcomeStep
            onBeginInstall={handleBeginInstall}
            onSkipToExternal={handleSkipToExternal}
            isInstalling={runtime.isInstalling || runtime.isRefreshing}
            downloadSizeBytes={
              decision?.localInstallSizeBytes ?? decision?.modelSizeBytes ?? null
            }
            isReturningUser={decision?.isReturningUser ?? false}
            localInstallSupported={runtime.status?.hardware?.supported ?? false}
            supportMessage={
              runtime.status?.hardware?.message ?? "正在检测本机翻译引擎支持情况…"
            }
          />
        )}
        {step === "installing-runtime" && (
          <InstallStep
            progress={runtime.progress}
            errorMessage={errorMessage}
            onCancel={handleCancel}
            onRetry={handleRetry}
            onSkip={handleSkipToExternal}
            defaultCaption={formatDownloadCaption(decision?.modelSizeBytes ?? null)}
            downloadingCaption={formatDownloadCaption(decision?.modelSizeBytes ?? null)}
            title="正在准备本地翻译引擎"
            stepLabel="步骤 1 / 2"
          />
        )}
        {step === "installing-pdf" && (
          <InstallStep
            progress={pdfRuntime.progress}
            errorMessage={errorMessage ?? pdfRuntime.lastError}
            onCancel={handleCancel}
            onRetry={handleRetry}
            onSkip={handleSkipPdf}
            title="正在准备 PDF 组件"
            errorTitle="PDF 组件没有准备完成"
            retryLabel="重试 PDF 安装"
            confirmCancelText="确认取消 PDF 安装？"
            defaultCaption="用于保留 PDF 页面结构，仅在本机运行"
            downloadingCaption="正在下载 PDF 版面处理组件"
            skipLabel="暂时跳过 PDF 组件"
            skipHint="以后可在设置中安装"
            stepLabel={usingExternalApi ? "可选组件" : "步骤 2 / 2"}
          />
        )}
        {step === "done" && (
          <DoneStep
            variant={doneVariant}
            onContinue={handleEnterWorkspace}
            isContinuing={isFinishing}
          />
        )}
      </div>
    </div>
  );
}

function toMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return JSON.stringify(error);
}
