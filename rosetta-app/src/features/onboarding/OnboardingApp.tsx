import { useCallback, useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { completeOnboardingAndOpenMain, getOnboardingDecision } from "@/lib/onboarding";
import {
  isPdf2zhReady,
  useManagedPdf2zhRuntime,
} from "@/lib/useManagedPdf2zhRuntime";
import {
  WINDOWS_LIGHTNING_PROFILE_ID,
  WINDOWS_LLAMACPP_PROFILE_ID,
  selectManagedRuntimeProfileStatus,
} from "@/lib/managedRuntimeSelection";
import {
  getPdf2zhWorkerStatus,
  prewarmPdf2zhWorker,
  subscribePdf2zhWorkerStatus,
  type Pdf2zhWorkerStatus,
} from "@/lib/pdf2zhRuntime";
import { useManagedRwkvRuntime } from "@/lib/useManagedRwkvRuntime";
import { useRosettaStore } from "@/store/useRosettaStore";
import { cn } from "@/lib/utils";
import type { ManagedRuntimeLogsSummary, ManagedRuntimeProfileStatus } from "@/types/rosetta";

import { DoneStep } from "./DoneStep";
import { InstallStep } from "./InstallStep";
import { WelcomeStep } from "./WelcomeStep";

type OnboardingStep =
  | "rwkv"
  | "installing-runtime"
  | "pdf"
  | "installing-pdf"
  | "welcome";

type OnboardingDebugState = {
  flow: string | null;
  lastAction: string | null;
  lastEventAt: string | null;
  installResult: string | null;
  startResult: string | null;
  probeResult: string | null;
  lastCaughtError: string | null;
};

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
  const updateRwkvConfig = useRosettaStore((state) => state.updateRwkvConfig);
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
  const [selectedRuntimeProfileId, setSelectedRuntimeProfileId] =
    useState<string | null>(null);
  const [rwkvLogs, setRwkvLogs] = useState<ManagedRuntimeLogsSummary | null>(
    null
  );
  const [debugState, setDebugState] = useState<OnboardingDebugState>({
    flow: null,
    lastAction: null,
    lastEventAt: null,
    installResult: null,
    startResult: null,
    probeResult: null,
    lastCaughtError: null,
  });
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

  const profileStatuses = runtime.status?.profileStatuses ?? [];
  const lightningStatus = findProfileStatus(
    profileStatuses,
    WINDOWS_LIGHTNING_PROFILE_ID
  );
  const llamaCppStatus = findProfileStatus(
    profileStatuses,
    WINDOWS_LLAMACPP_PROFILE_ID
  );
  const selectedRuntimeStatus = selectManagedRuntimeProfileStatus(
    runtime.status,
    selectedRuntimeProfileId
  );
  const selectedRuntimeProfile =
    selectedRuntimeStatus?.profile ?? runtime.status?.profile ?? null;
  const selectedModelSizeBytes =
    selectedRuntimeProfile?.modelSizeBytes ?? decision?.modelSizeBytes ?? null;
  const lightningSupported = lightningStatus?.hardware.supported ?? false;
  const showLlamaFallback =
    lightningSupported &&
    !!llamaCppStatus &&
    llamaCppStatus.state !== "unsupported";
  const selectedRuntimeIsLightning =
    selectedRuntimeProfile?.id === WINDOWS_LIGHTNING_PROFILE_ID;

  useEffect(() => {
    if (selectedRuntimeProfileId) {
      return;
    }
    const nextProfileId = selectedRuntimeStatus?.profile.id;
    if (!nextProfileId) {
      return;
    }
    setSelectedRuntimeProfileId(nextProfileId);
    updateRwkvConfig({ managedRuntimeProfileId: nextProfileId });
  }, [selectedRuntimeProfileId, selectedRuntimeStatus?.profile.id, updateRwkvConfig]);

  const beginPdfSetup = useCallback(async () => {
    setErrorMessage(null);
    setIsPrewarmingPdf(false);
    setStep("installing-pdf");
    setDebugState((prev) => ({
      ...prev,
      flow: "pdf-setup",
      lastAction: "beginPdfSetup",
      lastEventAt: isoNow(),
      lastCaughtError: null,
    }));

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
      const message = toMessage(error);
      setDebugState((prev) => ({
        ...prev,
        lastAction: "beginPdfSetup.catch",
        lastEventAt: isoNow(),
        lastCaughtError: message,
      }));
      setErrorMessage(message);
    } finally {
      setIsPrewarmingPdf(false);
    }
  }, [pdfRuntime]);

  const installLocalRuntime = useCallback(async (profileId?: string | null) => {
    const targetProfileId =
      profileId ?? selectedRuntimeProfileId ?? selectedRuntimeStatus?.profile.id;
    if (targetProfileId) {
      setSelectedRuntimeProfileId(targetProfileId);
      updateRwkvConfig({ managedRuntimeProfileId: targetProfileId });
    }

    if (rwkvInstallFlowActiveRef.current) {
      setDebugState((prev) => ({
        ...prev,
        flow: "rwkv-install",
        lastAction: "installLocalRuntime.ignoredActiveFlow",
        lastEventAt: isoNow(),
      }));
      return;
    }

    rwkvInstallFlowActiveRef.current = true;
    setErrorMessage(null);
    setRwkvLogs(null);
    setDebugState({
      flow: "rwkv-install",
      lastAction: "installLocalRuntime.begin",
      lastEventAt: isoNow(),
      installResult: null,
      startResult: null,
      probeResult: null,
      lastCaughtError: null,
    });
    setStep("installing-runtime");

    try {
      const installed = await runtime.install({
        repair: false,
        profileId: targetProfileId,
      });
      setDebugState((prev) => ({
        ...prev,
        lastAction: "runtime.install.returned",
        lastEventAt: isoNow(),
        installResult: summarizeValue(installed),
      }));
      if (!installed) {
        setErrorMessage(null);
        return;
      }

      const started = await runtime.start(targetProfileId);
      setDebugState((prev) => ({
        ...prev,
        lastAction: "runtime.start.returned",
        lastEventAt: isoNow(),
        startResult: summarizeValue(started),
      }));
      if (!started) {
        const logs = await runtime.readLogs(targetProfileId);
        setRwkvLogs(logs);
        setErrorMessage(null);
        return;
      }

      const probe = await runtime.probe(targetProfileId);
      setDebugState((prev) => ({
        ...prev,
        lastAction: "runtime.probe.returned",
        lastEventAt: isoNow(),
        probeResult: summarizeValue(probe),
      }));
      if (!probe?.ok) {
        const logs = await runtime.readLogs(targetProfileId);
        setRwkvLogs(logs);
        const message = probe?.message ?? "本地翻译引擎探活没有完成。";
        setDebugState((prev) => ({
          ...prev,
          lastAction: "runtime.probe.notOk",
          lastEventAt: isoNow(),
          lastCaughtError: message,
        }));
        setErrorMessage(message);
        return;
      }

      setSkippedLocalInstall(false);
      setStep("pdf");
    } catch (error) {
      const message = toMessage(error);
      const logs = await runtime.readLogs(targetProfileId);
      setRwkvLogs(logs);
      setDebugState((prev) => ({
        ...prev,
        lastAction: "installLocalRuntime.catch",
        lastEventAt: isoNow(),
        lastCaughtError: message,
      }));
      setErrorMessage(message);
    } finally {
      rwkvInstallFlowActiveRef.current = false;
    }
  }, [
    runtime,
    selectedRuntimeProfileId,
    selectedRuntimeStatus?.profile.id,
    updateRwkvConfig,
  ]);

  const handleBeginInstall = useCallback(() => {
    void installLocalRuntime(selectedRuntimeProfileId);
  }, [installLocalRuntime, selectedRuntimeProfileId]);

  const handleUseLlamaCpp = useCallback(() => {
    setSelectedRuntimeProfileId(WINDOWS_LLAMACPP_PROFILE_ID);
    updateRwkvConfig({ managedRuntimeProfileId: WINDOWS_LLAMACPP_PROFILE_ID });
    setErrorMessage(null);
    void installLocalRuntime(WINDOWS_LLAMACPP_PROFILE_ID);
  }, [installLocalRuntime, updateRwkvConfig]);

  const handleBeginPdfInstall = useCallback(() => {
    void beginPdfSetup();
  }, [beginPdfSetup]);

  const handleRetry = useCallback(() => {
    setErrorMessage(null);
    if (step === "installing-pdf") {
      void beginPdfSetup();
      return;
    }

    void installLocalRuntime(selectedRuntimeProfileId);
  }, [beginPdfSetup, installLocalRuntime, selectedRuntimeProfileId, step]);

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

  const localInstallSupport =
    selectedRuntimeStatus?.hardware ?? runtime.status?.hardware;
  const localInstallSupported = localInstallSupport?.supported ?? false;
  const isCheckingLocalInstallSupport =
    runtime.isRefreshing && !selectedRuntimeStatus;
  const runtimeDisplayName =
    selectedRuntimeProfile?.runtimeLabel ?? "RWKV local runtime";
  const rwkvTitle = decision?.isReturningUser
    ? `Download ${runtimeDisplayName}`
    : `Set up ${runtimeDisplayName}`;
  const rwkvDescription = decision?.isReturningUser
    ? "Install the current local translation runtime for this machine."
    : "Rosetta installs the runtime and translation model in local app data.";
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
  const runtimePrimaryLabel = isCheckingLocalInstallSupport
    ? "Checking local runtime support"
    : localInstallSupported
      ? `Install ${runtimeDisplayName}`
      : "Continue";
  const runtimePrimaryCaption = isCheckingLocalInstallSupport
    ? "Rosetta is checking whether this machine can run a managed local translator."
    : localInstallSupported
      ? formatDownloadCaption(selectedModelSizeBytes)
      : localInstallSupport?.message ??
        "This machine cannot install the selected local runtime. You can continue and configure a remote API later.";
  const llamaFallbackCaption =
    "Broader Windows compatibility. You can still switch back to Lightning later.";

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
            primaryLabel={runtimePrimaryLabel}
            primaryCaption={runtimePrimaryCaption}
            onPrimary={
              localInstallSupported ? handleBeginInstall : handleSkipLocalInstall
            }
            onSkip={handleSkipLocalInstall}
            isPrimaryDisabled={
              runtime.isInstalling ||
              (isCheckingLocalInstallSupport && !localInstallSupported)
            }
            primaryIcon={localInstallSupported ? "download" : "arrow"}
            secondaryLabel={
              showLlamaFallback && selectedRuntimeIsLightning
                ? "Use llama.cpp Vulkan instead"
                : undefined
            }
            secondaryCaption={
              showLlamaFallback && selectedRuntimeIsLightning
                ? llamaFallbackCaption
                : undefined
            }
            onSecondary={
              showLlamaFallback && selectedRuntimeIsLightning
                ? handleUseLlamaCpp
                : undefined
            }
            isSecondaryDisabled={runtime.isInstalling}
            skipLabel="暂时跳过 RWKV"
          />
        )}
        {step === "installing-runtime" && (
          <InstallStep
            progress={runtime.progress}
            errorMessage={errorMessage ?? runtime.lastError}
            logs={rwkvLogs}
            diagnostics={{
              component: "managed-rwkv",
              onboardingStep: step,
              flow: debugState.flow,
              lastAction: debugState.lastAction,
              lastEventAt: debugState.lastEventAt,
              installResult: debugState.installResult,
              startResult: debugState.startResult,
              probeResult: debugState.probeResult,
              lastCaughtError: debugState.lastCaughtError,
              isInstalling: runtime.isInstalling,
              isStarting: runtime.isStarting,
              isProbing: runtime.isProbing,
              isRefreshing: runtime.isRefreshing,
              statusState: selectedRuntimeStatus?.state,
              statusMessage: selectedRuntimeStatus?.message,
              runtimeProfile: selectedRuntimeStatus?.profile.id,
              provider: selectedRuntimeStatus?.profile.providerId,
              backend: selectedRuntimeStatus?.profile.backend,
              healthPath: selectedRuntimeStatus?.profile.batchChatPath,
              bindHost: selectedRuntimeStatus?.profile.bindHost,
              processPid: selectedRuntimeStatus?.process.pid,
              processBaseUrl: selectedRuntimeStatus?.process.baseUrl,
              processStartedAt: selectedRuntimeStatus?.process.startedAt,
              processCpuFallback: selectedRuntimeStatus?.process.cpuFallback,
              processLastError: selectedRuntimeStatus?.process.lastError,
              installPlanReady: selectedRuntimeStatus?.installPlan?.ready,
              installPlanMessage: selectedRuntimeStatus?.installPlan?.message,
              modelFile: selectedRuntimeStatus?.paths.modelFile,
              runtimeDir: selectedRuntimeStatus?.paths.runtimeDir,
              logsDir: selectedRuntimeStatus?.paths.logsDir,
            }}
            onCancel={handleCancel}
            onRetry={handleRetry}
            onFallback={
              showLlamaFallback && selectedRuntimeIsLightning
                ? handleUseLlamaCpp
                : undefined
            }
            fallbackLabel={
              showLlamaFallback && selectedRuntimeIsLightning
                ? "Install llama.cpp Vulkan instead"
                : undefined
            }
            fallbackDescription={
              showLlamaFallback && selectedRuntimeIsLightning
                ? llamaFallbackCaption
                : undefined
            }
            onSkip={handleSkipLocalInstall}
            progressValue={33}
            defaultCaption={formatDownloadCaption(selectedModelSizeBytes)}
            downloadingCaption={formatDownloadCaption(selectedModelSizeBytes)}
            title={`Preparing ${runtimeDisplayName}`}
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
            diagnostics={{
              component: "managed-pdf2zh",
              onboardingStep: step,
              flow: debugState.flow,
              lastAction: debugState.lastAction,
              lastEventAt: debugState.lastEventAt,
              lastCaughtError: debugState.lastCaughtError,
              statusState: pdfRuntime.status?.state,
              statusMessage: pdfRuntime.status?.message,
              installPlanReady: pdfRuntime.status?.installPlan?.ready,
              installPlanMessage: pdfRuntime.status?.installPlan?.message,
              packDir: pdfRuntime.status?.paths?.packDir,
              bin: pdfRuntime.status?.paths?.bin,
              logsDir: pdfRuntime.status?.paths?.logsDir,
              workerState: pdfWorkerStatus?.state,
              workerMessage: pdfWorkerStatus?.message,
              workerImportMs: pdfWorkerStatus?.importMs,
              warmupStep: pdfWorkerStatus?.warmupStep,
              warmupTotalSteps: pdfWorkerStatus?.warmupTotalSteps,
              warmupLabel: pdfWorkerStatus?.warmupLabel,
              warmupElapsedSeconds: pdfWarmupElapsed,
            }}
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

function findProfileStatus(
  profileStatuses: ManagedRuntimeProfileStatus[],
  profileId: string
): ManagedRuntimeProfileStatus | null {
  return (
    profileStatuses.find((entry) => entry.profile.id === profileId) ?? null
  );
}

function isoNow(): string {
  return new Date().toISOString();
}

function summarizeValue(value: unknown): string {
  if (value == null) return String(value);
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
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
