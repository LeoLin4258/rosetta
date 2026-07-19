import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  getRosettaPdfV3RunStatus,
  listRosettaPdfV3Runs,
} from "@/lib/rosettaJobs";
import type {
  PdfV3PageControlStatus,
  PdfV3RunControlStatus,
  PdfV3RunListItem,
  PdfV3RunState,
} from "@/types/rosetta";

const PDF_V3_STATUS_WINDOW_SIZE = 64;
const PDF_V3_STATUS_WINDOW_CACHE_LIMIT = 4;
const PDF_V3_STATUS_POLL_INTERVAL_MS = 1_000;
const PDF_V3_PAUSED_STATUS_POLL_INTERVAL_MS = 2_500;
const PDF_V3_RUN_DISCOVERY_INTERVAL_MS = 2_000;
const PDF_V3_RUN_DISCOVERY_WINDOW_MS = 30_000;

type StatusWindow = {
  fetchedAt: number;
  lastUsed: number;
  pages: PdfV3PageControlStatus[];
  runState: PdfV3RunState;
};

export function usePdfV3Preview({
  jobId,
  targetLanguage,
  visiblePageNumber,
  isTranslating,
}: {
  jobId: string;
  targetLanguage: string;
  visiblePageNumber: number | null;
  isTranslating: boolean;
}) {
  const contextKey = `${jobId}\u0000${targetLanguage}`;
  const [runSelection, setRunSelection] = useState<{
    contextKey: string;
    run: PdfV3RunListItem | null;
  }>({ contextKey, run: null });
  const run = runSelection.contextKey === contextKey ? runSelection.run : null;
  const [status, setStatus] = useState<PdfV3RunControlStatus | null>(null);
  const [pagesByNumber, setPagesByNumber] = useState<
    ReadonlyMap<number, PdfV3PageControlStatus>
  >(new Map());
  const [error, setError] = useState<string | null>(null);
  const [discoveryError, setDiscoveryError] = useState<string | null>(null);
  const [discovery, setDiscovery] = useState({ contextKey, active: true });
  const isDiscovering =
    discovery.contextKey === contextKey ? discovery.active : true;
  const selectedRunIdRef = useRef<string | null>(null);
  const statusWindowsRef = useRef<Map<number, StatusWindow>>(new Map());

  const resetSelectedRun = useCallback(
    (nextRun: PdfV3RunListItem | null) => {
      selectedRunIdRef.current = nextRun?.runId ?? null;
      statusWindowsRef.current.clear();
      setRunSelection({ contextKey, run: nextRun });
      setStatus(null);
      setPagesByNumber(new Map());
      setError(null);
      setDiscoveryError(null);
    },
    [contextKey],
  );

  useEffect(() => {
    resetSelectedRun(null);
    setDiscovery({ contextKey, active: true });
  }, [jobId, resetSelectedRun, targetLanguage]);

  useEffect(() => {
    let cancelled = false;
    let discoveryInFlight = false;

    async function discoverLatestRun() {
      if (discoveryInFlight) return;
      discoveryInFlight = true;
      try {
        const result = await listRosettaPdfV3Runs(jobId, {
          targetLanguage,
          limit: 1,
        });
        if (cancelled) return;
        const latestRun = result.runs[0] ?? null;
        if ((latestRun?.runId ?? null) !== selectedRunIdRef.current) {
          resetSelectedRun(latestRun);
        }
        setDiscoveryError(null);
      } catch (cause) {
        if (!cancelled) {
          console.error("[pdf-v3] failed to discover latest run", cause);
          setDiscoveryError("无法读取 PDF v3 运行状态。");
        }
      } finally {
        discoveryInFlight = false;
        if (!cancelled) setDiscovery({ contextKey, active: false });
      }
    }

    void discoverLatestRun();
    const discoveryDeadline = Date.now() + PDF_V3_RUN_DISCOVERY_WINDOW_MS;
    let interval = isTranslating
      ? window.setInterval(
          () => {
            if (Date.now() >= discoveryDeadline) {
              if (interval != null) window.clearInterval(interval);
              interval = null;
              return;
            }
            void discoverLatestRun();
          },
          PDF_V3_RUN_DISCOVERY_INTERVAL_MS,
        )
      : null;

    return () => {
      cancelled = true;
      if (interval != null) window.clearInterval(interval);
    };
  }, [jobId, isTranslating, resetSelectedRun, targetLanguage]);

  const windowStart = statusWindowStart(visiblePageNumber ?? 1);
  const runState = status?.state ?? run?.state ?? null;
  const runIsTerminal =
    runState === "cancelled" || runState === "completed";

  useEffect(() => {
    if (!run) return;
    const cachedWindow = statusWindowsRef.current.get(windowStart);
    if (runIsTerminal && cachedWindow?.runState === runState) {
      cachedWindow.lastUsed = Date.now();
      return;
    }
    const selectedRun = run;

    let cancelled = false;
    let refreshInFlight = false;

    async function refreshWindow() {
      if (refreshInFlight) return;
      refreshInFlight = true;
      try {
        const nextStatus = await getRosettaPdfV3RunStatus(
          jobId,
          selectedRun.runId,
          {
            startAfter: windowStart - 1 || undefined,
            limit: PDF_V3_STATUS_WINDOW_SIZE,
          },
        );
        if (cancelled || selectedRunIdRef.current !== selectedRun.runId) return;
        setStatus(nextStatus);
        setError(null);
        updateStatusWindow(
          statusWindowsRef.current,
          windowStart,
          nextStatus.pages,
          nextStatus.state,
        );
        setPagesByNumber(indexStatusWindows(statusWindowsRef.current));
      } catch (cause) {
        if (cancelled || selectedRunIdRef.current !== selectedRun.runId) return;
        console.error("[pdf-v3] failed to load visible page status", cause);
        setError("无法读取这次 PDF 翻译的页面状态。");
      } finally {
        refreshInFlight = false;
      }
    }

    void refreshWindow();
    const interval = runIsTerminal
      ? null
      : window.setInterval(
          () => void refreshWindow(),
          runState === "paused"
            ? PDF_V3_PAUSED_STATUS_POLL_INTERVAL_MS
            : PDF_V3_STATUS_POLL_INTERVAL_MS,
        );

    return () => {
      cancelled = true;
      if (interval != null) window.clearInterval(interval);
    };
  }, [jobId, run, runIsTerminal, runState, windowStart]);

  const requestedRanges = useMemo(
    () =>
      parseCanonicalPageSet(
        status?.requestedPageSet ?? run?.requestedPageSet ?? "",
      ),
    [run?.requestedPageSet, status?.requestedPageSet],
  );

  const isPageRequested = useCallback(
    (pageNumber: number) =>
      requestedRanges.some(
        ([start, end]) => pageNumber >= start && pageNumber <= end,
      ),
    [requestedRanges],
  );

  return {
    run,
    status,
    runState,
    pagesByNumber,
    isPageRequested,
    isDiscovering,
    discoveryError,
    error,
  };
}

function statusWindowStart(pageNumber: number) {
  const normalizedPage = Math.max(1, Math.trunc(pageNumber));
  return (
    Math.floor((normalizedPage - 1) / PDF_V3_STATUS_WINDOW_SIZE) *
      PDF_V3_STATUS_WINDOW_SIZE +
    1
  );
}

function updateStatusWindow(
  windows: Map<number, StatusWindow>,
  windowStart: number,
  pages: PdfV3PageControlStatus[],
  runState: PdfV3RunState,
) {
  const now = Date.now();
  windows.set(windowStart, { fetchedAt: now, lastUsed: now, pages, runState });
  while (windows.size > PDF_V3_STATUS_WINDOW_CACHE_LIMIT) {
    let oldestKey: number | null = null;
    let oldestUse = Number.POSITIVE_INFINITY;
    for (const [key, entry] of windows) {
      if (entry.lastUsed < oldestUse) {
        oldestKey = key;
        oldestUse = entry.lastUsed;
      }
    }
    if (oldestKey == null) break;
    windows.delete(oldestKey);
  }
}

function indexStatusWindows(windows: ReadonlyMap<number, StatusWindow>) {
  const pages = new Map<number, PdfV3PageControlStatus>();
  const orderedWindows = [...windows.values()].sort(
    (left, right) => left.fetchedAt - right.fetchedAt,
  );
  for (const window of orderedWindows) {
    for (const page of window.pages) pages.set(page.pageNumber, page);
  }
  return pages;
}

function parseCanonicalPageSet(value: string): Array<readonly [number, number]> {
  const ranges: Array<readonly [number, number]> = [];
  for (const part of value.split(",")) {
    const normalized = part.trim();
    if (!normalized) continue;
    const [startValue, endValue = startValue] = normalized.split("-", 2);
    const start = Number(startValue);
    const end = Number(endValue);
    if (
      Number.isSafeInteger(start) &&
      Number.isSafeInteger(end) &&
      start > 0 &&
      end >= start
    ) {
      ranges.push([start, end]);
    }
  }
  return ranges;
}
