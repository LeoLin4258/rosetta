import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  cancelRosettaPdfV3Run,
  createRosettaPdfV3Run,
  getRosettaPdfV3RunStatus,
  listRosettaPdfV3Runs,
  pauseRosettaPdfV3Run,
  recoverRosettaPdfV3Run,
  resumeRosettaPdfV3Run,
  retryRosettaPdfV3Page,
} from "@/lib/rosettaJobs";
import type { PdfV3RunControlStatus } from "@/types/rosetta";

const ACTIVE_STATUS_POLL_INTERVAL_MS = 1_000;
const QUIESCENT_STATUS_POLL_INTERVAL_MS = 2_500;

export type PdfV3RunOperation =
  | "creating"
  | "pausing"
  | "resuming"
  | "cancelling"
  | "recovering"
  | "retrying";

type ContextualValue<T> = {
  contextKey: string;
  value: T;
};

export function usePdfV3RunControl({
  jobId,
  targetLanguage,
  enabled,
}: {
  jobId: string | null;
  targetLanguage: string;
  enabled: boolean;
}) {
  const contextKey = `${jobId ?? ""}\u0000${targetLanguage}`;
  const contextKeyRef = useRef(contextKey);
  contextKeyRef.current = contextKey;

  const [statusState, setStatusState] = useState<
    ContextualValue<PdfV3RunControlStatus | null>
  >({ contextKey, value: null });
  const [operationState, setOperationState] = useState<
    ContextualValue<PdfV3RunOperation | null>
  >({ contextKey, value: null });
  const [discoveryState, setDiscoveryState] = useState<
    ContextualValue<boolean>
  >({ contextKey, value: enabled });
  const [errorState, setErrorState] = useState<
    ContextualValue<string | null>
  >({ contextKey, value: null });
  const [discoveryErrorState, setDiscoveryErrorState] = useState<
    ContextualValue<string | null>
  >({ contextKey, value: null });
  const [recoveryNowMs, setRecoveryNowMs] = useState(() => Date.now());

  const status = statusState.contextKey === contextKey ? statusState.value : null;
  const operation =
    operationState.contextKey === contextKey ? operationState.value : null;
  const isDiscovering =
    discoveryState.contextKey === contextKey
      ? discoveryState.value
      : enabled;
  const error = errorState.contextKey === contextKey ? errorState.value : null;
  const discoveryError =
    discoveryErrorState.contextKey === contextKey
      ? discoveryErrorState.value
      : null;

  const applyStatus = useCallback(
    (nextStatus: PdfV3RunControlStatus | null) => {
      if (contextKeyRef.current !== contextKey) return;
      setStatusState({ contextKey, value: nextStatus });
    },
    [contextKey],
  );

  useEffect(() => {
    setStatusState({ contextKey, value: null });
    setOperationState({ contextKey, value: null });
    setDiscoveryState({ contextKey, value: enabled });
    setErrorState({ contextKey, value: null });
    setDiscoveryErrorState({ contextKey, value: null });
  }, [contextKey, enabled]);

  useEffect(() => {
    if (!enabled || !jobId) return;
    let cancelled = false;

    async function discover() {
      try {
        const runs = await listRosettaPdfV3Runs(jobId!, {
          targetLanguage,
          limit: 1,
        });
        if (cancelled || contextKeyRef.current !== contextKey) return;
        const latestRun = runs.runs[0] ?? null;
        if (!latestRun) {
          applyStatus(null);
          setErrorState({ contextKey, value: null });
          setDiscoveryErrorState({ contextKey, value: null });
          return;
        }
        const nextStatus = await getRosettaPdfV3RunStatus(
          jobId!,
          latestRun.runId,
          { limit: 1 },
        );
        if (cancelled || contextKeyRef.current !== contextKey) return;
        applyStatus(nextStatus);
        setErrorState({ contextKey, value: null });
        setDiscoveryErrorState({ contextKey, value: null });
      } catch (cause) {
        if (cancelled || contextKeyRef.current !== contextKey) return;
        console.error("[pdf-v3] failed to discover run control state", cause);
        setErrorState({
          contextKey,
          value: "无法读取 PDF v3 运行状态。",
        });
        setDiscoveryErrorState({
          contextKey,
          value: "无法读取 PDF v3 运行状态。",
        });
      } finally {
        if (!cancelled && contextKeyRef.current === contextKey) {
          setDiscoveryState({ contextKey, value: false });
        }
      }
    }

    void discover();
    return () => {
      cancelled = true;
    };
  }, [applyStatus, contextKey, enabled, jobId, targetLanguage]);

  const runIsTerminal =
    status?.state === "cancelled" ||
    status?.state === "failed" ||
    status?.state === "completed";

  useEffect(() => {
    if (
      !status ||
      status.ownedByCurrentSession ||
      status.state === "cancelled" ||
      status.state === "completed"
    ) {
      return;
    }
    const remainingMs = status.ownerRecoveryEligibleAtMs - Date.now();
    if (remainingMs <= 0) {
      setRecoveryNowMs(Date.now());
      return;
    }
    const timeout = window.setTimeout(
      () => setRecoveryNowMs(Date.now()),
      remainingMs + 50,
    );
    return () => window.clearTimeout(timeout);
  }, [
    status?.ownedByCurrentSession,
    status?.ownerRecoveryEligibleAtMs,
    status?.state,
  ]);

  useEffect(() => {
    if (!enabled || !jobId || !status || runIsTerminal || operation) return;
    const selectedRunId = status.runId;
    const selectedRunState = status.state;
    let cancelled = false;
    let inFlight = false;

    async function refresh() {
      if (inFlight) return;
      inFlight = true;
      try {
        const nextStatus = await getRosettaPdfV3RunStatus(
          jobId!,
          selectedRunId,
          { limit: 1 },
        );
        if (cancelled || contextKeyRef.current !== contextKey) return;
        applyStatus(nextStatus);
        setErrorState({ contextKey, value: null });
      } catch (cause) {
        if (cancelled || contextKeyRef.current !== contextKey) return;
        console.error("[pdf-v3] failed to refresh run control state", cause);
        setErrorState({
          contextKey,
          value: "PDF v3 运行状态暂时无法刷新。",
        });
      } finally {
        inFlight = false;
      }
    }

    const interval = window.setInterval(
      () => void refresh(),
      selectedRunState === "running" || selectedRunState === "cancelling"
        ? ACTIVE_STATUS_POLL_INTERVAL_MS
        : QUIESCENT_STATUS_POLL_INTERVAL_MS,
    );
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [
    applyStatus,
    contextKey,
    enabled,
    jobId,
    operation,
    runIsTerminal,
    status?.runId,
    status?.state,
  ]);

  const execute = useCallback(
    async <T extends PdfV3RunControlStatus>(
      nextOperation: PdfV3RunOperation,
      task: () => Promise<T>,
    ) => {
      setOperationState({ contextKey, value: nextOperation });
      setErrorState({ contextKey, value: null });
      try {
        const nextStatus = await task();
        if (contextKeyRef.current === contextKey) {
          applyStatus(nextStatus);
          setErrorState({ contextKey, value: null });
        }
        return nextStatus;
      } catch (cause) {
        if (contextKeyRef.current === contextKey) {
          setErrorState({
            contextKey,
            value:
              cause instanceof Error
                ? cause.message
                : typeof cause === "string" && cause.trim()
                  ? cause
                  : "PDF v3 操作失败。",
          });
        }
        throw cause;
      } finally {
        if (contextKeyRef.current === contextKey) {
          setOperationState({ contextKey, value: null });
        }
      }
    },
    [applyStatus, contextKey],
  );

  const create = useCallback(
    async (requestedPageSet: string, preferredPageNumber: number | null) => {
      if (!jobId) throw new Error("PDF 项目不可用。");
      return execute("creating", () =>
        createRosettaPdfV3Run(
          jobId,
          requestedPageSet,
          targetLanguage,
          preferredPageNumber,
        ),
      );
    },
    [execute, jobId, targetLanguage],
  );

  const pause = useCallback(async () => {
    if (!jobId || !status) throw new Error("PDF v3 运行不可用。");
    return execute("pausing", () =>
      pauseRosettaPdfV3Run(jobId, status.runId),
    );
  }, [execute, jobId, status]);

  const resume = useCallback(async () => {
    if (!jobId || !status) throw new Error("PDF v3 运行不可用。");
    return execute("resuming", () =>
      resumeRosettaPdfV3Run(jobId, status.runId),
    );
  }, [execute, jobId, status]);

  const cancel = useCallback(async () => {
    if (!jobId || !status) throw new Error("PDF v3 运行不可用。");
    return execute("cancelling", () =>
      cancelRosettaPdfV3Run(jobId, status.runId),
    );
  }, [execute, jobId, status]);

  const recover = useCallback(async () => {
    if (!jobId || !status) throw new Error("PDF v3 运行不可用。");
    return execute("recovering", async () => {
      const result = await recoverRosettaPdfV3Run(jobId, status.runId);
      return result.status;
    });
  }, [execute, jobId, status]);

  const retryPage = useCallback(
    async (pageNumber: number) => {
      if (!jobId || !status) throw new Error("PDF v3 运行不可用。");
      return execute("retrying", () =>
        retryRosettaPdfV3Page(jobId, status.runId, pageNumber),
      );
    },
    [execute, jobId, status],
  );

  const isOwned = status?.ownedByCurrentSession ?? false;
  const canRecover =
    !!status &&
    status.state !== "cancelled" &&
    status.state !== "completed" &&
    !isOwned &&
    recoveryNowMs >= status.ownerRecoveryEligibleAtMs;
  const completedPages = status
    ? status.summary.completedPages + status.summary.preservedPages
    : 0;

  return useMemo(
    () => ({
      status,
      operation,
      isDiscovering,
      error,
      discoveryError,
      isOwned,
      canRecover,
      runIsTerminal,
      completedPages,
      applyStatus,
      create,
      pause,
      resume,
      cancel,
      recover,
      retryPage,
    }),
    [
      applyStatus,
      canRecover,
      cancel,
      completedPages,
      create,
      error,
      discoveryError,
      isDiscovering,
      isOwned,
      operation,
      pause,
      recover,
      resume,
      retryPage,
      runIsTerminal,
      status,
    ],
  );
}
