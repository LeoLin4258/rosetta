import { useCallback, useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { completeOnboardingAndOpenMain, getOnboardingDecision } from "@/lib/onboarding";
import {
  isPdf2zhReady,
  useManagedPdf2zhRuntime,
} from "@/lib/useManagedPdf2zhRuntime";
import {
  getPdf2zhWorkerStatus,
  prewarmPdf2zhWorker,
  subscribePdf2zhWorkerStatus,
  type Pdf2zhWorkerStatus,
} from "@/lib/pdf2zhRuntime";
import { useManagedRwkvRuntime } from "@/lib/useManagedRwkvRuntime";
import { cn } from "@/lib/utils";

import { DoneStep } from "./DoneStep";
import { InstallStep } from "./InstallStep";
import { WelcomeStep } from "./WelcomeStep";

type OnboardingStep =
  | "rwkv"
  | "installing-runtime"
  | "pdf"
  | "installing-pdf"
  | "welcome";

const appWindow = getCurrentWindow();

function formatDownloadCaption(modelSizeBytes: number | null): string {
  if (modelSizeBytes == null || modelSizeBytes <= 0) {
    return "下载完成后无需再联网";
  }

  const mb = modelSizeBytes / (1024 * 1024);
  const label =
    mb >= 1024
      ? `约 ${(mb / 1024).toFixed(1)} GB`
      : `约 ${Math.round(mb)} MB`;
  return `${label} · 下载完成后无需再联网`;
}

/**
 * Root of the onboarding window. The order is fixed and single-direction:
 * RWKV -> PDF -> Welcome.
 *
 * Each step can be skipped, but skipping only ever moves forward:
 * - Skip RWKV -> PDF
 * - Skip PDF -> Welcome
 * - Cancel an active download -> back to that step's choice screen
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

  const [step, setStep] = useState<OnboardingStep>("rwkv");
  const [skippedLocalInstall, setSkippedLocalInstall] = useState(false);
  const [skippedPdfInstall, setSkippedPdfInstall] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [isFinishing, setIsFinishing] = useState(false);
  const [isPrewarmingPdf, setIsPrewarmingPdf] = useState(false);
  const [pdfWorkerStatus, setPdfWorkerStatus] =
    useState<Pdf2zhWorkerStatus | null>(null);
  const [pdfWarmupElapsed, setPdfWarmupElapsed] = useState(0);
  const rwkvInstallFlowActiveRef = useRef(false);
  const [decision, setDecision] = useState<{
    modelSizeBytes: number | null;
    isReturningUser: boolean;
  } | null>(null);

  useEffect(() => {
    let mounted = true;
    void getOnboardingDecision()
      .then((next) => {
        if (!mounted) {
          return;
        }

        setDecision({
          modelSizeBytes: next.modelSizeBytes,
          isReturningUser: next.isReturningUser,
        });
      })
      .catch(() => {
        // Best-effort: let the UI fall back to generic copy.
      });

    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | null = null;

    void getPdf2zhWorkerStatus()
      .then((status) => {
        if (active) {
          setPdfWorkerStatus(status);
        }
      })
      .catch(() => {});

    subscribePdf2zhWorkerStatus((status) => {
      if (active) {
        setPdfWorkerStatus(status);
      }
    })
      .then((nextUnlisten) => {
        if (!active) {
          nextUnlisten();
          return;
        }
        unlisten = nextUnlisten;
      })
      .catch(() => {});

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    if (!isPrewarmingPdf) {
      setPdfWarmupElapsed(0);
      return;
    }

    const startedAt = Date.now();
    const interval = window.setInterval(() => {
      setPdfWarmupElapsed(Math.floor((Date.now() - startedAt) / 1000));
    }, 1000);
    return () => window.clearInterval(interval);
  }, [isPrewarmingPdf]);

  const beginPdfSetup = useCallback(async () => {
    setErrorMessage(null);
    setIsPrewarmingPdf(false);
    setStep("installing-pdf");

    try {
      const existing = await pdfRuntime.refreshStatus();
      if (!isPdf2zhReady(existing)) {
        await pdfRuntime.install({ repair: false });
      }

      setIsPrewarmingPdf(true);
      const warmed = await prewarmPdf2zhWorker();
      if (!warmed) {
        throw new Error("PDF 组件已安装，但预热没有完成。请重试。");
      }

      setSkippedPdfInstall(false);
      setStep("welcome");
    } catch (error) {
      setErrorMessage(toMessage(error));
    } finally {
      setIsPrewarmingPdf(false);
    }
  }, [pdfRuntime]);

  const installLocalRuntime = useCallback(async () => {
    if (rwkvInstallFlowActiveRef.current) {
      return;
    }

    rwkvInstallFlowActiveRef.current = true;
    setErrorMessage(null);
    setStep("installing-runtime");

    try {
      const installed = await runtime.install({ repair: false });
      if (!installed) {
        setErrorMessage(null);
        return;
      }

      const started = await runtime.start();
      if (!started) {
        setErrorMessage(null);
        return;
      }

      const probe = await runtime.probe();
      if (!probe?.ok) {
        setErrorMessage(probe?.message ?? "本地翻译引擎探活没有完成。");
        return;
      }

      setSkippedLocalInstall(false);
      setStep("pdf");
    } finally {
      rwkvInstallFlowActiveRef.current = false;
    }
  }, [runtime]);

  const handleBeginInstall = useCallback(() => {
    void installLocalRuntime();
  }, [installLocalRuntime]);

  const handleBeginPdfInstall = useCallback(() => {
    void beginPdfSetup();
  }, [beginPdfSetup]);

  const handleRetry = useCallback(() => {
    setErrorMessage(null);
    if (step === "installing-pdf") {
      void beginPdfSetup();
      return;
    }

    void installLocalRuntime();
  }, [beginPdfSetup, installLocalRuntime, step]);

  const handleCancel = useCallback(() => {
    if (step === "installing-pdf") {
      void pdfRuntime.cancelInstall();
      setStep("pdf");
    } else {
      void runtime.cancelInstall();
      setStep("rwkv");
    }

    setErrorMessage(null);
  }, [pdfRuntime, runtime, step]);

  const handleSkipLocalInstall = useCallback(() => {
    setSkippedLocalInstall(true);
    setErrorMessage(null);
    setStep("pdf");
  }, []);

  const handleSkipPdf = useCallback(() => {
    setSkippedPdfInstall(true);
    setErrorMessage(null);
    setStep("welcome");
  }, []);

  const handleEnterWorkspace = useCallback(async () => {
    setIsFinishing(true);
    try {
      await completeOnboardingAndOpenMain({
        skippedLocalInstall,
      });
    } catch (error) {
      console.error("complete onboarding failed", error);
      setIsFinishing(false);
    }
  }, [skippedLocalInstall]);

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

  const localInstallSupport = runtime.status?.hardware;
  const localInstallSupported = localInstallSupport?.supported ?? false;
  const isCheckingLocalInstallSupport =
    runtime.isRefreshing && localInstallSupport == null;
  const rwkvTitle = decision?.isReturningUser
    ? "下载新的 RWKV 翻译引擎"
    : "下载 RWKV 翻译引擎";
  const rwkvDescription = decision?.isReturningUser
    ? "新版本已切换到更小更快的本地模型。"
    : "翻译引擎和模型会安装在本机。";
  const pdfProgress = isPrewarmingPdf
    ? {
        phase: "preparing" as const,
        bytesDone: 0,
        bytesTotal: 0,
        speedBytesPerSec: 0,
        message: formatPdfWarmupMessage(pdfWorkerStatus, pdfWarmupElapsed),
        lastError: null,
      }
    : pdfRuntime.progress;

  return (
    <div
      className={cn(
        "rosetta-onboarding flex h-screen flex-col select-none bg-transparent text-foreground",
        systemPrefersDark && "dark"
      )}
    >
      <div
        className="h-10 w-full shrink-0"
        data-tauri-drag-region
        onMouseDown={handleDragStripMouseDown}
      />
      <div className="min-h-0 flex-1">
        {step === "rwkv" && (
          <WelcomeStep
            stepLabel="步骤 1 / 3"
            progressValue={33}
            title={rwkvTitle}
            description={rwkvDescription}
            primaryLabel={
              isCheckingLocalInstallSupport
                ? "正在检测本机支持"
                : localInstallSupported
                ? decision?.isReturningUser
                  ? "下载新模型"
                  : "安装本地翻译引擎"
                : "继续下一步"
            }
            primaryCaption={
              isCheckingLocalInstallSupport
                ? "正在检测本机翻译引擎支持情况…"
                : localInstallSupported
                ? formatDownloadCaption(decision?.modelSizeBytes ?? null)
                : localInstallSupport?.message ??
                  "当前设备不支持本地翻译引擎，可先继续下一步。"
            }
            onPrimary={
              localInstallSupported ? handleBeginInstall : handleSkipLocalInstall
            }
            onSkip={handleSkipLocalInstall}
            isPrimaryDisabled={
              runtime.isInstalling ||
              (isCheckingLocalInstallSupport && !localInstallSupported)
            }
            primaryIcon={localInstallSupported ? "download" : "arrow"}
            skipLabel="暂时跳过 RWKV"
          />
        )}
        {step === "installing-runtime" && (
          <InstallStep
            progress={runtime.progress}
            errorMessage={errorMessage ?? runtime.lastError}
            onCancel={handleCancel}
            onRetry={handleRetry}
            onSkip={handleSkipLocalInstall}
            progressValue={33}
            defaultCaption={formatDownloadCaption(decision?.modelSizeBytes ?? null)}
            downloadingCaption={formatDownloadCaption(decision?.modelSizeBytes ?? null)}
            title="正在准备本地翻译引擎"
            stepLabel="步骤 1 / 3"
            skipLabel="跳过 RWKV，继续下一步"
          />
        )}
        {step === "pdf" && (
          <WelcomeStep
            stepLabel="步骤 2 / 3"
            progressValue={66}
            title="下载 PDF 组件"
            description="用于保留 PDF 页面结构。以后也可以在设置里补装。"
            primaryLabel="安装 PDF 组件"
            primaryCaption="仅在处理 PDF 文档时需要，整个流程仍然只在本机运行。"
            onPrimary={handleBeginPdfInstall}
            onSkip={handleSkipPdf}
            isPrimaryDisabled={pdfRuntime.isInstalling || pdfRuntime.isRefreshing}
            primaryIcon="download"
            skipLabel="暂时跳过 PDF"
            showProxyConfig={false}
          />
        )}
        {step === "installing-pdf" && (
          <InstallStep
            progress={pdfProgress}
            errorMessage={errorMessage ?? pdfRuntime.lastError}
            onCancel={handleCancel}
            onRetry={handleRetry}
            onSkip={handleSkipPdf}
            progressValue={66}
            title={isPrewarmingPdf ? "正在启动 PDF 引擎" : "正在准备 PDF 组件"}
            errorTitle="PDF 组件没有准备完成"
            retryLabel="重试 PDF 安装"
            defaultCaption="用于保留 PDF 页面结构，仅在本机运行"
            downloadingCaption="正在下载 PDF 版面处理组件"
            skipLabel="暂时跳过 PDF 组件"
            stepLabel="步骤 2 / 3"
          />
        )}
        {step === "welcome" && (
          <DoneStep
            skippedLocalInstall={skippedLocalInstall}
            skippedPdfInstall={skippedPdfInstall}
            onContinue={handleEnterWorkspace}
            isContinuing={isFinishing}
          />
        )}
      </div>
    </div>
  );
}

function toMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  return JSON.stringify(error);
}

function formatPdfWarmupMessage(
  status: Pdf2zhWorkerStatus | null,
  elapsedSeconds: number
): string {
  const details: string[] = [];
  if (
    status?.state === "starting" &&
    status.warmupStep != null &&
    status.warmupTotalSteps != null &&
    status.warmupLabel
  ) {
    details.push(
      `第 ${status.warmupStep}/${status.warmupTotalSteps} 阶段：${status.warmupLabel}`
    );
  } else {
    details.push("正在启动本机 PDF 处理进程");
  }
  if (elapsedSeconds > 0) {
    details.push(`已用 ${elapsedSeconds} 秒`);
  }
  return details.join(" · ");
}
