import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { getRosettaPdfV3RunStatus } from "@/lib/rosettaJobs";
import type {
  PdfV3PageControlStatus,
  PdfV3RunControlStatus,
  PdfV3RunState,
} from "@/types/rosetta";

const PDF_V3_STATUS_WINDOW_SIZE = 64;
const PDF_V3_STATUS_WINDOW_CACHE_LIMIT = 4;

type StatusWindow = {
  fetchedAt: number;
  lastUsed: number;
  pages: PdfV3PageControlStatus[];
  runState: PdfV3RunState;
};

export function usePdfV3Preview({
  jobId,
  runStatus,
  visiblePageNumber,
  isDiscovering,
  discoveryError,
}: {
  jobId: string;
  runStatus: PdfV3RunControlStatus | null;
  visiblePageNumber: number | null;
  isDiscovering: boolean;
  discoveryError: string | null;
}) {
  const [pagesByNumber, setPagesByNumber] = useState<
    ReadonlyMap<number, PdfV3PageControlStatus>
  >(new Map());
  const [error, setError] = useState<string | null>(null);
  const statusWindowsRef = useRef<Map<number, StatusWindow>>(new Map());
  const runId = runStatus?.runId ?? null;

  useEffect(() => {
    statusWindowsRef.current.clear();
    setPagesByNumber(new Map());
    setError(null);
  }, [jobId, runId]);

  const windowStart = statusWindowStart(visiblePageNumber ?? 1);
  const runState = runStatus?.state ?? null;
  const statusRefreshKey = runStatus
    ? [
        runStatus.state,
        runStatus.summary.pendingPages,
        runStatus.summary.extractingPages,
        runStatus.summary.extractedPages,
        runStatus.summary.translatingPages,
        runStatus.summary.completedPages,
        runStatus.summary.preservedPages,
        runStatus.summary.failedPages,
      ].join(":")
    : null;
  const runIsTerminal =
    runState === "cancelled" || runState === "completed";

  useEffect(() => {
    if (!runStatus) return;
    const cachedWindow = statusWindowsRef.current.get(windowStart);
    if (runIsTerminal && cachedWindow?.runState === runState) {
      cachedWindow.lastUsed = Date.now();
      return;
    }
    const selectedRunId = runStatus.runId;
    let cancelled = false;

    async function refreshWindow() {
      try {
        const nextStatus = await getRosettaPdfV3RunStatus(
          jobId,
          selectedRunId,
          {
            startAfter: windowStart - 1 || undefined,
            limit: PDF_V3_STATUS_WINDOW_SIZE,
          },
        );
        if (cancelled || nextStatus.runId !== selectedRunId) return;
        setError(null);
        updateStatusWindow(
          statusWindowsRef.current,
          windowStart,
          nextStatus.pages,
          nextStatus.state,
        );
        setPagesByNumber(indexStatusWindows(statusWindowsRef.current));
      } catch (cause) {
        if (cancelled) return;
        console.error("[pdf-v3] failed to load visible page status", cause);
        setError("无法读取这次 PDF 翻译的页面状态。");
      }
    }

    void refreshWindow();
    return () => {
      cancelled = true;
    };
  }, [
    jobId,
    runId,
    runIsTerminal,
    runState,
    statusRefreshKey,
    windowStart,
  ]);

  const requestedRanges = useMemo(
    () => parseCanonicalPageSet(runStatus?.requestedPageSet ?? ""),
    [runStatus?.requestedPageSet],
  );

  const isPageRequested = useCallback(
    (pageNumber: number) =>
      requestedRanges.some(
        ([start, end]) => pageNumber >= start && pageNumber <= end,
      ),
    [requestedRanges],
  );

  return {
    run: runStatus,
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
