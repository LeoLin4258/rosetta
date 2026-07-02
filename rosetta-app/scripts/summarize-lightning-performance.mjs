#!/usr/bin/env node

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const DEFAULT_APP_DATA =
  process.platform === "win32" && process.env.APPDATA
    ? path.join(process.env.APPDATA, "com.rosetta.desktop")
    : path.join(os.homedir(), ".rosetta");

function usage() {
  return `Usage:
  node scripts/summarize-lightning-performance.mjs [options]

Options:
  --perf-log <path>   rwkv-performance.jsonl path
                     default: <app-data>/logs/rwkv-performance.jsonl
  --profile <path>    optional PDF profile JSON path
  --job-id <id>       keep records whose context contains this job id
  --run-id <id>       keep records whose context contains this run id
  --context <text>    keep records whose context contains this text
  --output <path>     write summary JSON to this path
  --help              show this help

The performance log records counts and timings only. It does not include source
text, translated text, prompt bodies, or raw model responses.`;
}

function parseArgs(argv) {
  const options = {
    appData: DEFAULT_APP_DATA,
  };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      options.help = true;
      continue;
    }
    if (!arg.startsWith("--")) {
      throw new Error(`Unexpected argument: ${arg}`);
    }
    const key = arg.slice(2);
    const value = argv[index + 1];
    if (!value || value.startsWith("--")) {
      throw new Error(`Missing value for ${arg}`);
    }
    index += 1;
    options[key.replace(/-([a-z])/g, (_, char) => char.toUpperCase())] = value;
  }
  options.perfLog ??= path.join(options.appData, "logs", "rwkv-performance.jsonl");
  return options;
}

function readJsonl(filePath) {
  if (!fs.existsSync(filePath)) {
    return [];
  }
  return fs
    .readFileSync(filePath, "utf8")
    .split(/\r?\n/)
    .filter((line) => line.trim().length > 0)
    .map((line, index) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        throw new Error(`${filePath}:${index + 1}: ${error.message}`);
      }
    });
}

function percentile(values, percentileValue) {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.ceil((sorted.length * percentileValue) / 100);
  return sorted[Math.max(0, rank - 1)];
}

function round(value, digits = 2) {
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}

function histogram(records, field) {
  const counts = new Map();
  for (const record of records) {
    const key = Number(record[field] ?? 0);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([value, count]) => ({ value, count }));
}

function summarizeRecords(records) {
  const successful = records.filter((record) => record.ok);
  const failed = records.filter((record) => !record.ok);
  const timestamps = records
    .map((record) => Number(record.timestampMs))
    .filter(Number.isFinite);
  const latencies = records.map((record) => Number(record.latencyMs ?? 0));
  const requestSeconds =
    records.reduce((sum, record) => sum + Number(record.latencyMs ?? 0), 0) / 1000;
  const wallSeconds =
    timestamps.length > 1
      ? (Math.max(...timestamps) - Math.min(...timestamps)) / 1000
      : requestSeconds;
  const inputChars = records.reduce(
    (sum, record) => sum + Number(record.inputChars ?? 0),
    0,
  );
  const outputChars = records.reduce(
    (sum, record) => sum + Number(record.outputChars ?? 0),
    0,
  );

  return {
    requestCount: records.length,
    successfulRequestCount: successful.length,
    failedRequestCount: failed.length,
    batchSizeDistribution: histogram(records, "batchSize").map(({ value, count }) => ({
      batchSize: value,
      requestCount: count,
    })),
    totalInputChars: inputChars,
    totalOutputChars: outputChars,
    latencyMs: {
      average: records.length ? round(latencies.reduce((a, b) => a + b, 0) / records.length) : 0,
      median: percentile(latencies, 50),
      p95: percentile(latencies, 95),
      max: latencies.length ? Math.max(...latencies) : 0,
    },
    overheadMs: {
      prepareRequestTotal: records.reduce(
        (sum, record) => sum + Number(record.prepareRequestMs ?? 0),
        0,
      ),
      httpSendTotal: records.reduce((sum, record) => sum + Number(record.httpSendMs ?? 0), 0),
      responseReadTotal: records.reduce(
        (sum, record) => sum + Number(record.responseReadMs ?? 0),
        0,
      ),
      responseParseTotal: records.reduce(
        (sum, record) => sum + Number(record.responseParseMs ?? 0),
        0,
      ),
    },
    throughput: {
      observedWallSeconds: round(wallSeconds, 3),
      summedRequestSeconds: round(requestSeconds, 3),
      sourceCharsPerWallSecond: wallSeconds > 0 ? round(inputChars / wallSeconds) : 0,
      outputCharsPerWallSecond: wallSeconds > 0 ? round(outputChars / wallSeconds) : 0,
      sourceCharsPerRequestSecond: requestSeconds > 0 ? round(inputChars / requestSeconds) : 0,
      outputCharsPerRequestSecond: requestSeconds > 0 ? round(outputChars / requestSeconds) : 0,
    },
    errors: failed.map((record) => ({
      timestampMs: record.timestampMs,
      context: record.context,
      statusCode: record.statusCode,
      error: record.error,
    })),
  };
}

function loadProfile(profilePath) {
  if (!profilePath) return null;
  const profile = JSON.parse(fs.readFileSync(profilePath, "utf8"));
  return {
    path: profilePath,
    runId: profile.runId,
    jobId: profile.jobId,
    status: profile.status,
    sourceLang: profile.sourceLang,
    targetLang: profile.targetLang,
    pageSelection: profile.pageSelection,
    pagesRequested: profile.pagesRequested,
    pagesTranslated: profile.pagesTranslated,
    pagesFailed: profile.pagesFailed,
    invocationCount: profile.invocationCount,
    durationsMs: profile.durationsMs,
    rwkv: profile.rwkv ?? null,
  };
}

function summarizeTimeline(profilePath, runId) {
  if (!profilePath || !runId) return null;
  const timelinePath = path.join(path.dirname(profilePath), "pdf-timeline.jsonl");
  if (!fs.existsSync(timelinePath)) return null;
  const events = readJsonl(timelinePath).filter((event) => event.runId === runId);
  const workerStages = new Map();
  const translationDurations = new Map();

  for (const event of events) {
    const durationMs = Number(event.durationMs);
    if (!Number.isFinite(durationMs)) continue;

    if (event.event === "worker.stage") {
      const stage = String(event.details?.stage ?? "unknown");
      const current = workerStages.get(stage) ?? {
        stage,
        count: 0,
        totalMs: 0,
        maxMs: 0,
      };
      current.count += 1;
      current.totalMs += durationMs;
      current.maxMs = Math.max(current.maxMs, durationMs);
      workerStages.set(stage, current);
      continue;
    }

    if (event.phase === "translation") {
      const name = String(event.event ?? "unknown");
      const current = translationDurations.get(name) ?? {
        event: name,
        count: 0,
        totalMs: 0,
        maxMs: 0,
      };
      current.count += 1;
      current.totalMs += durationMs;
      current.maxMs = Math.max(current.maxMs, durationMs);
      translationDurations.set(name, current);
    }
  }

  const finalizeDuration = (entry) => ({
    ...entry,
    totalMs: round(entry.totalMs, 2),
    averageMs: entry.count > 0 ? round(entry.totalMs / entry.count, 2) : 0,
    maxMs: round(entry.maxMs, 2),
  });

  const crossPageBatch = events
    .filter(
      (event) =>
        event.event === "worker.stage" &&
        String(event.details?.stage ?? "").startsWith("crossPageBatch.") &&
        event.details?.status === "completed",
    )
    .map((event) => ({
      stage: event.details?.stage,
      durationMs: event.durationMs ?? null,
      details: event.details?.stageDetails ?? null,
    }));

  return {
    path: timelinePath,
    eventCount: events.length,
    workerStageDurations: [...workerStages.values()]
      .map(finalizeDuration)
      .sort((a, b) => b.totalMs - a.totalMs),
    translationDurations: [...translationDurations.values()]
      .map(finalizeDuration)
      .sort((a, b) => b.totalMs - a.totalMs),
    crossPageBatch,
  };
}

function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log(usage());
    return;
  }

  const allRecords = readJsonl(options.perfLog).filter(
    (record) => record.provider === "rwkv-lightning-contents",
  );
  const records = allRecords.filter((record) => {
    const context = String(record.context ?? "");
    if (options.jobId && !context.includes(options.jobId)) return false;
    if (options.runId && !context.includes(options.runId)) return false;
    if (options.context && !context.includes(options.context)) return false;
    return true;
  });
  const profile = loadProfile(options.profile);
  const timeline = summarizeTimeline(options.profile, profile?.runId);
  const summary = {
    generatedAt: new Date().toISOString(),
    perfLog: options.perfLog,
    filters: {
      jobId: options.jobId ?? null,
      runId: options.runId ?? null,
      context: options.context ?? null,
    },
    recordCountBeforeFilters: allRecords.length,
    performance: summarizeRecords(records),
    pdfProfile: profile,
    pdfTimeline: timeline,
  };

  const output = `${JSON.stringify(summary, null, 2)}\n`;
  if (options.output) {
    fs.writeFileSync(options.output, output);
  }
  process.stdout.write(output);
}

try {
  main();
} catch (error) {
  console.error(error.message);
  console.error("");
  console.error(usage());
  process.exitCode = 1;
}
