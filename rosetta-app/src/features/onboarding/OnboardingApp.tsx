import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { completeOnboardingAndOpenMain, getOnboardingDecision } from "@/lib/onboarding";
import {
  isPdf2zhReady,
  useManagedPdf2zhRuntime,
} from "@/lib/useManagedPdf2zhRuntime";
import { useManagedRwkvRuntime } from "@/lib/useManagedRwkvRuntime";
import { cn } from "@/lib/utils";

import { DoneStep } from "./DoneStep";
import { InstallStep } from "./InstallStep";
import { PdfSetupStep } from "./PdfSetupStep";
import { WelcomeStep } from "./WelcomeStep";

type OnboardingStep =
  | "welcome"
  | "installing-runtime"
  | "pdf-setup"
  | "installing-pdf"
  | "done";

const appWindow = getCurrentWindow();

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
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [pdfErrorMessage, setPdfErrorMessage] = useState<string | null>(null);
  const [isFinishing, setIsFinishing] = useState(false);
  // Pull the decision once to feed Welcome step the model size + "are we
  // upgrading" flag. `null` while loading and on errors — Welcome falls
  // back to neutral copy in that case rather than blocking the screen.
  const [decision, setDecision] = useState<{
    modelSizeBytes: number | null;
    isReturningUser: boolean;
  } | null>(null);
  useEffect(() => {
    let mounted = true;
    void getOnboardingDecision()
      .then((d) => {
        if (!mounted) return;
        setDecision({
          modelSizeBytes: d.modelSizeBytes,
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

  // React to runtime status updates that arrive after we kicked off install:
  // success → step "done", failure → stay on "installing" with error banner.
  useEffect(() => {
    if (step !== "installing-runtime") return;
    if (runtime.isInstalling) return;
    if (runtime.lastError) {
      setErrorMessage(runtime.lastError);
      return;
    }
    // Success criterion: runtime status report says model is installed.
    if (
      runtime.status?.state === "installed" ||
      runtime.status?.state === "ready"
    ) {
      void enterPdfSetup();
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step, runtime.isInstalling, runtime.lastError, runtime.status?.state]);

  const enterPdfSetup = useCallback(async () => {
    setPdfErrorMessage(null);
    const status = await pdfRuntime.refreshStatus();
    if (isPdf2zhReady(status)) {
      setDoneVariant("local");
      setStep("done");
      return;
    }
    setStep("pdf-setup");
  }, [pdfRuntime]);

  const beginPdfInstall = useCallback(async () => {
    setPdfErrorMessage(null);
    setStep("installing-pdf");
    try {
      const status = await pdfRuntime.refreshStatus();
      if (!isPdf2zhReady(status)) {
        await pdfRuntime.install({ repair: false });
        const refreshed = await pdfRuntime.refreshStatus();
        if (!isPdf2zhReady(refreshed)) {
          throw new Error(
            refreshed?.message ??
              "PDF 版面处理组件安装完成后仍未就绪，请稍后在设置中检查。"
          );
        }
      }
      setDoneVariant("local");
      setStep("done");
    } catch (error) {
      setPdfErrorMessage(toMessage(error));
    }
  }, [pdfRuntime]);

  const handleBeginInstall = useCallback(() => {
    setErrorMessage(null);
    setPdfErrorMessage(null);
    setStep("installing-runtime");
    // useManagedRwkvRuntime.install() reads `downloadProxy.url` from store
    // and merges into the options automatically (see useManagedRwkvRuntime),
    // so the proxy the user typed in WelcomeStep is honoured here without
    // explicit threading.
    void runtime.install({ repair: false });
  }, [runtime]);

  const handleRetry = useCallback(() => {
    setErrorMessage(null);
    void runtime.install({ repair: false });
  }, [runtime]);

  const handleRetryPdf = useCallback(() => {
    void beginPdfInstall();
  }, [beginPdfInstall]);

  const handleCancel = useCallback(() => {
    void runtime.cancelInstall();
    setStep("welcome");
    setErrorMessage(null);
  }, [runtime]);

  const handleCancelPdf = useCallback(() => {
    void pdfRuntime.cancelInstall();
    setStep("done");
    setDoneVariant("local-pdf-skipped");
    setPdfErrorMessage(null);
  }, [pdfRuntime]);

  const handleSkipPdf = useCallback(() => {
    setDoneVariant("local-pdf-skipped");
    setStep("done");
    setPdfErrorMessage(null);
  }, []);

  const handleSkipToExternal = useCallback(async () => {
    setIsFinishing(true);
    setDoneVariant("external");
    try {
      await completeOnboardingAndOpenMain({ skippedLocalInstall: true });
      // Tauri command closes onboarding window + shows main; nothing else to do.
    } catch (error) {
      console.error("complete onboarding (skip) failed", error);
      setIsFinishing(false);
    }
  }, []);

  const handleEnterWorkspace = useCallback(async () => {
    setIsFinishing(true);
    try {
      await completeOnboardingAndOpenMain({ skippedLocalInstall: false });
    } catch (error) {
      console.error("complete onboarding failed", error);
      setIsFinishing(false);
    }
  }, []);

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
            isInstalling={runtime.isInstalling}
            modelSizeBytes={decision?.modelSizeBytes ?? null}
            isReturningUser={decision?.isReturningUser ?? false}
          />
        )}
        {step === "installing-runtime" && (
          <InstallStep
            progress={runtime.progress}
            errorMessage={errorMessage}
            onCancel={handleCancel}
            onRetry={handleRetry}
            onSkip={handleSkipToExternal}
          />
        )}
        {step === "pdf-setup" && (
          <PdfSetupStep
            onBeginInstall={beginPdfInstall}
            onSkip={handleSkipPdf}
            isInstalling={pdfRuntime.isInstalling}
          />
        )}
        {step === "installing-pdf" && (
          <InstallStep
            title="正在准备 PDF 版面处理"
            errorTitle="PDF 版面处理没有安装完成"
            retryLabel="重新准备"
            cancelLabel="稍后再装"
            confirmCancelText="确认稍后再准备 PDF 版面处理？"
            continueLabel="继续准备"
            defaultCaption="用于保留 PDF 排版并生成译文 PDF"
            downloadingCaption="下载完成后，PDF 翻译可以保留原文排版"
            skipLabel="暂时跳过 PDF 版面处理"
            skipHint="之后可在设置中安装"
            progress={pdfRuntime.progress}
            errorMessage={pdfErrorMessage}
            onCancel={handleCancelPdf}
            onRetry={handleRetryPdf}
            onSkip={handleSkipPdf}
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
