#!/usr/bin/env node

import { readdir, readFile, stat, writeFile } from "node:fs/promises";
import path from "node:path";

const DEFAULT_TARGET_LANG = "zh-CN";
const DEFAULT_APP_DATA_SUBDIR = "com.rosetta.desktop";
const DEFAULT_TIME_SLOP_MS = 5_000;

function parseArgs(argv) {
  const options = {
    appData: defaultAppDataRoot(),
    jobId: null,
    runId: null,
    targetLang: DEFAULT_TARGET_LANG,
    profile: null,
    rwkvLog: null,
    debugContext: null,
    maxTotalMs: null,
    output: null,
    allowMissingRwkvDebug: false,
    timeSlopMs: DEFAULT_TIME_SLOP_MS,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = () => {
      const value = argv[index + 1];
      if (!value || value.startsWith("--")) {
        throw new Error(`${arg} requires a value`);
      }
      index += 1;
      return value;
    };

    if (arg === "--app-data") options.appData = next();
    else if (arg === "--job-id") options.jobId = next();
    else if (arg === "--run-id") options.runId = next();
    else if (arg === "--target-lang") options.targetLang = next();
    else if (arg === "--profile") options.profile = next();
    else if (arg === "--rwkv-log") options.rwkvLog = next();
    else if (arg === "--debug-context") options.debugContext = next();
    else if (arg === "--max-total-ms") options.maxTotalMs = parsePositiveInt(next(), arg);
    else if (arg === "--output") options.output = next();
    else if (arg === "--time-slop-ms") options.timeSlopMs = parsePositiveInt(next(), arg);
    else if (arg === "--allow-missing-rwkv-debug") options.allowMissingRwkvDebug = true;
    else if (arg === "--help" || arg === "-h") {
      printHelp();
      process.exit(0);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  if (!options.profile && !options.jobId) {
    throw new Error("Provide --job-id or --profile.");
  }
  return options;
}

function defaultAppDataRoot() {
  if (process.env.APPDATA) {
    return path.join(process.env.APPDATA, DEFAULT_APP_DATA_SUBDIR);
  }
  if (process.env.HOME) {
    return path.join(process.env.HOME, ".local", "share", DEFAULT_APP_DATA_SUBDIR);
  }
  return DEFAULT_APP_DATA_SUBDIR;
}

function parsePositiveInt(raw, name) {
  const value = Number.parseInt(raw, 10);
  if (!Number.isFinite(value) || value <= 0) {
    throw new Error(`${name} must be a positive integer`);
  }
  return value;
}

function printHelp() {
  console.log(`Usage:
  node scripts/check-pdf-translation-run.mjs --job-id <job> [options]

Options:
  --app-data <path>       Rosetta app data root, default %APPDATA%/com.rosetta.desktop
  --job-id <id>           PDF job id
  --run-id <id>           profile run id; defaults to latest profile for the job
  --target-lang <lang>    target language for pdf_pages.<lang>.json, default zh-CN
  --profile <path>        explicit pdf-translation-profile-*.json path
  --rwkv-log <path>       rwkv-io-debug.jsonl path, default app-data/logs/rwkv-io-debug.jsonl
  --debug-context <ctx>   explicit rwkv-io-debug context; default pdf-job:<job-id>
  --max-total-ms <n>      fail if profile durationsMs.total exceeds n
  --output <path>         write machine-readable JSON summary
  --allow-missing-rwkv-debug
                          do not fail when rwkv-io-debug records are unavailable
  --time-slop-ms <n>      timestamp slack around profile start/end, default 5000
`);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const profilePath = options.profile
    ? path.resolve(options.profile)
    : await findProfilePath(options);
  const profile = await readJson(profilePath);
  const jobId = options.jobId ?? profile.jobId;
  if (!jobId) {
    throw new Error("Could not determine job id from arguments or profile.");
  }
  const targetLang = options.targetLang || profile.targetLang || DEFAULT_TARGET_LANG;
  const jobDir = path.join(options.appData, "jobs", jobId);
  const pageStatePath = path.join(jobDir, `pdf_pages.${targetLang}.json`);
  const timelinePath = path.join(jobDir, "diagnostics", "pdf-timeline.jsonl");
  const rwkvLogPath =
    options.rwkvLog ?? path.join(options.appData, "logs", "rwkv-io-debug.jsonl");

  const pageState = await readJsonIfExists(pageStatePath);
  const timeline = await readJsonlIfExists(timelinePath);
  const debugRecords = await readRwkvDebugRecords(rwkvLogPath, profile, jobId, options);
  const completionSummary = summarizeCompletions(debugRecords);
  const timelineSummary = summarizeTimeline(timeline, profile);
  const pageSummary = summarizePages(pageState, profile);

  const summary = {
    createdAt: new Date().toISOString(),
    jobId,
    runId: profile.runId,
    targetLang,
    paths: {
      profile: profilePath,
      pageState: pageState ? pageStatePath : null,
      timeline: timeline.length > 0 ? timelinePath : null,
      rwkvLog: debugRecords.length > 0 ? rwkvLogPath : null,
    },
    profile: summarizeProfile(profile),
    pages: pageSummary,
    timeline: timelineSummary,
    completions: completionSummary,
    failures: [],
  };

  summary.failures = collectFailures(summary, options);
  printSummary(summary);

  if (options.output) {
    await writeFile(options.output, `${JSON.stringify(summary, null, 2)}\n`, "utf8");
    console.log(`\nWrote ${options.output}`);
  }

  if (summary.failures.length > 0) {
    process.exitCode = 1;
  }
}

async function findProfilePath(options) {
  const diagnosticsDir = path.join(options.appData, "jobs", options.jobId, "diagnostics");
  if (options.runId) {
    return path.join(diagnosticsDir, `pdf-translation-profile-${options.runId}.json`);
  }

  const entries = await readdir(diagnosticsDir);
  const candidates = await Promise.all(
    entries
      .filter((name) => /^pdf-translation-profile-.+\.json$/u.test(name))
      .map(async (name) => {
        const fullPath = path.join(diagnosticsDir, name);
        const metadata = await stat(fullPath);
        let startedAt = 0;
        try {
          startedAt = Number((await readJson(fullPath)).startedAt ?? 0);
        } catch {
          // Fall back to mtime below.
        }
        return { path: fullPath, startedAt, mtimeMs: metadata.mtimeMs };
      }),
  );
  if (candidates.length === 0) {
    throw new Error(`No PDF translation profile found in ${diagnosticsDir}`);
  }
  candidates.sort((left, right) => {
    const leftTime = left.startedAt || left.mtimeMs;
    const rightTime = right.startedAt || right.mtimeMs;
    return rightTime - leftTime;
  });
  return candidates[0].path;
}

async function readJson(filePath) {
  const text = await readFile(filePath, "utf8");
  return JSON.parse(text);
}

async function readJsonIfExists(filePath) {
  try {
    return await readJson(filePath);
  } catch (error) {
    if (error?.code === "ENOENT") return null;
    throw error;
  }
}

async function readJsonlIfExists(filePath) {
  let text = "";
  try {
    text = await readFile(filePath, "utf8");
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }
  return text
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line, index) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        throw new Error(`Invalid JSONL in ${filePath} at line ${index + 1}: ${error.message}`);
      }
    });
}

async function readRwkvDebugRecords(logPath, profile, jobId, options) {
  let records = [];
  try {
    records = await readJsonlIfExists(logPath);
  } catch (error) {
    if (error?.code === "ENOENT") return [];
    throw error;
  }
  const context = `pdf-job:${jobId}`;
  const startedAt = Number(profile.startedAt ?? 0);
  const endedAt = Number(profile.endedAt ?? 0);
  const hasTimeRange = Number.isFinite(startedAt) && startedAt > 0 && Number.isFinite(endedAt) && endedAt > 0;
  const expectedContext = options.debugContext ?? context;
  const matchingContext = records.filter((record) => record.context === expectedContext);
  const candidates =
    matchingContext.length > 0
      ? matchingContext
      : records.filter((record) => String(record.context ?? "").includes(jobId));
  return candidates
    .filter((record) => record.provider === "llama-cpp-chat-completions")
    .filter((record) => {
      if (!hasTimeRange) return true;
      const timestamp = Number(record.timestampMs ?? 0);
      return (
        timestamp >= startedAt - options.timeSlopMs &&
        timestamp <= endedAt + options.timeSlopMs
      );
    });
}

function summarizeProfile(profile) {
  return {
    status: profile.status ?? null,
    pageSelection: profile.pageSelection ?? null,
    pagesRequested: Number(profile.pagesRequested ?? 0),
    pagesTranslated: Number(profile.pagesTranslated ?? 0),
    pagesFailed: Number(profile.pagesFailed ?? 0),
    totalMs: Number(profile.durationsMs?.total ?? 0),
    pdf2zhProcessMs: Number(profile.durationsMs?.pdf2zhProcess ?? 0),
    invocationCount: Number(profile.invocationCount ?? 0),
    shimBatches: Number(profile.rwkv?.requestCount ?? 0),
    shimFailedBatches: Number(profile.rwkv?.failedRequestCount ?? 0),
    shimAverageBatchMs: Number(profile.rwkv?.averageRequestMs ?? 0),
    shimMaxBatchMs: Number(profile.rwkv?.maxRequestMs ?? 0),
    shimInputChars: Number(profile.rwkv?.totalInputChars ?? 0),
    shimOutputChars: Number(profile.rwkv?.totalOutputChars ?? 0),
  };
}

function summarizePages(pageState, profile) {
  if (!pageState) {
    return {
      pageStatePresent: false,
      requestedPages: [],
      translatedRequestedPages: 0,
      failedRequestedPages: 0,
      missingRequestedPages: 0,
    };
  }
  const requestedPages = parsePageSelection(
    profile.pageSelection ?? "",
    Number(pageState.sourcePageCount ?? 0),
  );
  const byPage = new Map((pageState.pages ?? []).map((page) => [Number(page.pageNumber), page]));
  const statuses = requestedPages.map((pageNumber) => ({
    pageNumber,
    status: byPage.get(pageNumber)?.status ?? "missing",
  }));
  return {
    pageStatePresent: true,
    sourcePageCount: Number(pageState.sourcePageCount ?? 0),
    requestedPages,
    translatedRequestedPages: statuses.filter((page) => page.status === "translated").length,
    failedRequestedPages: statuses.filter((page) => page.status === "failed").length,
    missingRequestedPages: statuses.filter((page) => page.status === "missing").length,
    nonTranslatedRequestedPages: statuses.filter((page) => page.status !== "translated"),
  };
}

function parsePageSelection(selection, pageCount) {
  const trimmed = String(selection || "").trim();
  if (!trimmed || trimmed.toLowerCase() === "all") {
    return range(1, pageCount);
  }
  const pages = new Set();
  for (const part of trimmed.split(",")) {
    const token = part.trim();
    if (!token) continue;
    const rangeMatch = token.match(/^(\d+)\s*-\s*(\d+)$/u);
    if (rangeMatch) {
      const start = Number(rangeMatch[1]);
      const end = Number(rangeMatch[2]);
      for (const page of range(Math.min(start, end), Math.max(start, end))) {
        pages.add(page);
      }
      continue;
    }
    const page = Number(token);
    if (Number.isInteger(page) && page > 0) {
      pages.add(page);
    }
  }
  return [...pages].sort((left, right) => left - right);
}

function range(start, end) {
  if (!Number.isFinite(start) || !Number.isFinite(end) || end < start) return [];
  return Array.from({ length: end - start + 1 }, (_, index) => start + index);
}

function summarizeTimeline(events, profile) {
  const runEvents = events.filter((event) => event.runId === profile.runId);
  const pageCommits = runEvents
    .filter((event) => event.event === "page.committed")
    .map((event) => ({
      pageNumber: event.pageNumber ?? null,
      timestampMs: Number(event.timestampMs ?? 0),
      elapsedMs: Number(event.timestampMs ?? 0) - Number(profile.startedAt ?? 0),
    }))
    .filter((event) => event.timestampMs > 0)
    .sort((left, right) => left.timestampMs - right.timestampMs);
  const translateRequestDurations = runEvents
    .filter((event) => event.event === "worker.stage")
    .filter((event) => event.details?.stage === "page.processPage.translateRequest")
    .map((event) => Number(event.durationMs ?? event.details?.durationMs ?? 0))
    .filter((value) => value > 0);
  return {
    eventCount: runEvents.length,
    firstPageCommitMs: pageCommits[0]?.elapsedMs ?? null,
    pageCommits,
    translateRequestCount: translateRequestDurations.length,
    translateRequestLatencyMs: percentileSummary(translateRequestDurations),
  };
}

function summarizeCompletions(records) {
  const parsed = records.map((record, index) => parseCompletionRecord(record, index));
  const okRecords = parsed.filter((record) => record.ok);
  const promptTokens = sum(parsed.map((record) => record.promptTokens));
  const predictedTokens = sum(parsed.map((record) => record.predictedTokens));
  const promptMs = sum(parsed.map((record) => record.promptMs));
  const predictedMs = sum(parsed.map((record) => record.predictedMs));
  const wallMs = completionWallMs(parsed);
  return {
    recordCount: parsed.length,
    okCount: parsed.filter((record) => record.ok).length,
    failedCount: parsed.filter((record) => !record.ok).length,
    emptyOutputCount: parsed.filter((record) => record.emptyOutput).length,
    truncatedCount: parsed.filter((record) => record.truncated).length,
    limitStopCount: parsed.filter((record) => record.stopType === "limit").length,
    inputChars: sum(parsed.map((record) => record.inputChars)),
    outputChars: sum(parsed.map((record) => record.outputChars)),
    promptTokens,
    predictedTokens,
    totalTokens: promptTokens + predictedTokens,
    promptMs,
    predictedMs,
    estimatedWallMs: wallMs,
    promptTokensPerSecond: rate(promptTokens, wallMs),
    predictedTokensPerSecond: rate(predictedTokens, wallMs),
    completionLatencyMs: percentileSummary(parsed.map((record) => record.latencyMs).filter((value) => value > 0)),
    failures: parsed
      .filter(
        (record) =>
          !record.ok || record.emptyOutput || record.truncated || record.stopType === "limit",
      )
      .slice(0, 20)
      .map((record) => ({
        index: record.index,
        timestampMs: record.timestampMs,
        ok: record.ok,
        statusCode: record.statusCode,
        truncated: record.truncated,
        stopType: record.stopType,
        emptyOutput: record.emptyOutput,
        error: record.error,
      })),
  };
}

function parseCompletionRecord(record, index) {
  const raw = parseJsonLoose(record.rawResponse);
  const timings = raw?.timings ?? {};
  const promptMs = Number(timings.prompt_ms ?? 0);
  const predictedMs = Number(timings.predicted_ms ?? 0);
  const outputs = Array.isArray(record.outputs) ? record.outputs : [];
  const inputs = Array.isArray(record.inputs) ? record.inputs : [];
  const outputChars = outputs.join("\n").trim().length;
  return {
    index,
    timestampMs: Number(record.timestampMs ?? 0),
    ok: Boolean(record.ok),
    statusCode: record.statusCode ?? null,
    error: record.error ?? null,
    inputChars: inputs.join("\n").length,
    outputChars,
    emptyOutput: outputChars === 0,
    truncated: Boolean(raw?.truncated ?? false),
    stopType: raw?.stop_type ?? null,
    promptTokens: Number(timings.prompt_n ?? raw?.tokens_evaluated ?? 0),
    predictedTokens: Number(timings.predicted_n ?? raw?.tokens_predicted ?? 0),
    promptMs,
    predictedMs,
    latencyMs: promptMs + predictedMs,
  };
}

function parseJsonLoose(text) {
  if (!text || typeof text !== "string") return null;
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

function completionWallMs(records) {
  if (records.length === 0) return 0;
  const timed = records.filter((record) => record.timestampMs > 0 && record.latencyMs > 0);
  if (timed.length === 0) return 0;
  const firstStart = Math.min(
    ...timed.map((record) => record.timestampMs - record.latencyMs),
  );
  const lastEnd = Math.max(...timed.map((record) => record.timestampMs));
  return Math.max(0, lastEnd - firstStart);
}

function collectFailures(summary, options) {
  const failures = [];
  if (summary.profile.status !== "completed") {
    failures.push(`profile status is ${summary.profile.status ?? "missing"}, expected completed`);
  }
  if (summary.profile.pagesFailed !== 0) {
    failures.push(`profile pagesFailed is ${summary.profile.pagesFailed}, expected 0`);
  }
  if (summary.profile.shimFailedBatches !== 0) {
    failures.push(`shim failedRequestCount is ${summary.profile.shimFailedBatches}, expected 0`);
  }
  if (!summary.pages.pageStatePresent) {
    failures.push("pdf_pages.<targetLang>.json is missing");
  } else if (summary.pages.nonTranslatedRequestedPages.length > 0) {
    failures.push(
      `${summary.pages.nonTranslatedRequestedPages.length} requested page(s) are not translated`,
    );
  }
  if (!options.allowMissingRwkvDebug && summary.completions.recordCount === 0) {
    failures.push("no matching llama.cpp rwkv-io-debug records found; set ROSETTA_RWKV_IO_DEBUG=1 before running the benchmark");
  }
  if (summary.completions.failedCount !== 0) {
    failures.push(`${summary.completions.failedCount} completion record(s) have ok=false`);
  }
  if (summary.completions.emptyOutputCount !== 0) {
    failures.push(`${summary.completions.emptyOutputCount} completion record(s) have empty output`);
  }
  if (summary.completions.truncatedCount !== 0) {
    failures.push(`${summary.completions.truncatedCount} completion record(s) have truncated=true`);
  }
  if (summary.completions.limitStopCount !== 0) {
    failures.push(`${summary.completions.limitStopCount} completion record(s) have stop_type=limit`);
  }
  if (options.maxTotalMs !== null && summary.profile.totalMs > options.maxTotalMs) {
    failures.push(`total ${summary.profile.totalMs} ms exceeds limit ${options.maxTotalMs} ms`);
  }
  return failures;
}

function printSummary(summary) {
  console.log("PDF translation run check");
  console.log(`  job: ${summary.jobId}`);
  console.log(`  run: ${summary.runId}`);
  console.log(`  status: ${summary.profile.status}`);
  console.log(
    `  pages: requested=${summary.profile.pagesRequested}, translated=${summary.profile.pagesTranslated}, failed=${summary.profile.pagesFailed}`,
  );
  console.log(
    `  total: ${formatMs(summary.profile.totalMs)}, first page: ${formatNullableMs(summary.timeline.firstPageCommitMs)}`,
  );
  console.log(
    `  shim batches: ${summary.profile.shimBatches}, avg=${formatMs(summary.profile.shimAverageBatchMs)}, max=${formatMs(summary.profile.shimMaxBatchMs)}`,
  );
  console.log(
    `  completions: records=${summary.completions.recordCount}, ok=${summary.completions.okCount}, failed=${summary.completions.failedCount}, truncated=${summary.completions.truncatedCount}, limit=${summary.completions.limitStopCount}, empty=${summary.completions.emptyOutputCount}`,
  );
  console.log(
    `  completion latency p50/p95/max: ${formatMs(summary.completions.completionLatencyMs.p50)} / ${formatMs(summary.completions.completionLatencyMs.p95)} / ${formatMs(summary.completions.completionLatencyMs.max)}`,
  );
  console.log(
    `  throughput: prompt=${summary.completions.promptTokensPerSecond.toFixed(2)} tok/s, predicted=${summary.completions.predictedTokensPerSecond.toFixed(2)} tok/s`,
  );
  if (summary.failures.length === 0) {
    console.log("  result: PASS");
  } else {
    console.log("  result: FAIL");
    for (const failure of summary.failures) {
      console.log(`  - ${failure}`);
    }
    if (summary.completions.failures.length > 0) {
      console.log("  first completion failures:");
      for (const failure of summary.completions.failures.slice(0, 5)) {
        console.log(
          `  - #${failure.index} ts=${failure.timestampMs} ok=${failure.ok} status=${failure.statusCode} truncated=${failure.truncated} stop=${failure.stopType ?? "n/a"} empty=${failure.emptyOutput}`,
        );
      }
    }
  }
}

function percentileSummary(values) {
  if (values.length === 0) {
    return { min: 0, p50: 0, p90: 0, p95: 0, p99: 0, max: 0, avg: 0 };
  }
  const sorted = [...values].sort((left, right) => left - right);
  return {
    min: sorted[0],
    p50: percentile(sorted, 0.5),
    p90: percentile(sorted, 0.9),
    p95: percentile(sorted, 0.95),
    p99: percentile(sorted, 0.99),
    max: sorted[sorted.length - 1],
    avg: sum(sorted) / sorted.length,
  };
}

function percentile(sorted, p) {
  const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * p) - 1);
  return sorted[Math.max(0, index)];
}

function sum(values) {
  return values.reduce((total, value) => total + Number(value || 0), 0);
}

function rate(count, ms) {
  return ms > 0 ? count / (ms / 1000) : 0;
}

function formatMs(ms) {
  return `${Number(ms || 0).toFixed(0)} ms`;
}

function formatNullableMs(ms) {
  return ms === null || ms === undefined ? "n/a" : formatMs(ms);
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exitCode = 1;
});
