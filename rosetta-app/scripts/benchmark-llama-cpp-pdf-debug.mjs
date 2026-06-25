#!/usr/bin/env node

import { readFile, writeFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";

const DEFAULT_LOG_PATH =
  "C:/Users/Leo/AppData/Roaming/com.rosetta.desktop/logs/rwkv-io-debug.jsonl";
const DEFAULT_URL = "http://127.0.0.1:57808/completion";
const DEFAULT_CONCURRENCY = 4;
const DEFAULT_TIMEOUT_MS = 120_000;
const DEFAULT_TARGET_LANG = "zh-CN";
const MAX_TOKENS_PER_SEGMENT = 1024;

function parseArgs(argv) {
  const options = {
    log: DEFAULT_LOG_PATH,
    url: DEFAULT_URL,
    context: "latest",
    concurrency: DEFAULT_CONCURRENCY,
    timeoutMs: DEFAULT_TIMEOUT_MS,
    targetLang: DEFAULT_TARGET_LANG,
    limit: null,
    output: null,
    dryRun: false,
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

    if (arg === "--log") options.log = next();
    else if (arg === "--url") options.url = next();
    else if (arg === "--context") options.context = next();
    else if (arg === "--concurrency") options.concurrency = parsePositiveInt(next(), arg);
    else if (arg === "--timeout-ms") options.timeoutMs = parsePositiveInt(next(), arg);
    else if (arg === "--target-lang") options.targetLang = next();
    else if (arg === "--limit") options.limit = parsePositiveInt(next(), arg);
    else if (arg === "--output") options.output = next();
    else if (arg === "--dry-run") options.dryRun = true;
    else if (arg === "--help" || arg === "-h") {
      printHelp();
      process.exit(0);
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return options;
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
  node scripts/benchmark-llama-cpp-pdf-debug.mjs [options]

Options:
  --log <path>           rwkv-io-debug.jsonl path
  --url <url>            llama.cpp /completion URL
  --context <id|latest|all>
  --concurrency <n>      concurrent /completion requests per wave, default 4
  --timeout-ms <n>       per-request timeout, default 120000
  --target-lang <lang>   prompt target language, default zh-CN
  --limit <n>            benchmark only the first n source records
  --output <path>        write detailed JSON result
  --dry-run              parse and summarize only; do not call llama.cpp
`);
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const allRecords = await readJsonl(options.log);
  const records = selectRecords(allRecords, options.context);
  const usable = records
    .filter((record) => record.provider === "llama-cpp-chat-completions")
    .filter((record) => record.ok)
    .filter((record) => Array.isArray(record.inputs) && record.inputs.length > 0);

  const selected = options.limit ? usable.slice(0, options.limit) : usable;
  if (selected.length === 0) {
    throw new Error("No usable llama-cpp-chat-completions records found.");
  }

  const appBaseline = summarizeAppRecords(selected);
  const result = {
    createdAt: new Date().toISOString(),
    options,
    source: {
      log: options.log,
      context: summarizeContexts(selected),
      records: selected.length,
      inputChars: selected.reduce(
        (sum, record) => sum + record.inputs.join("").length,
        0,
      ),
    },
    appBaseline,
    bareRun: null,
  };

  printSummary("App baseline from debug log", appBaseline);

  if (!options.dryRun) {
    console.log(
      `\nBare run: ${selected.length} record(s), concurrency=${options.concurrency}, url=${options.url}`,
    );
    const bareRun = await runBenchmark(selected, options);
    result.bareRun = bareRun;
    printSummary("Bare llama.cpp replay", bareRun.summary);
    printComparison(appBaseline, bareRun.summary);
  }

  if (options.output) {
    await writeFile(options.output, `${JSON.stringify(result, null, 2)}\n`, "utf8");
    console.log(`\nWrote ${options.output}`);
  }
}

async function readJsonl(path) {
  const content = await readFile(path, "utf8");
  return content
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line, lineIndex) => {
      try {
        return JSON.parse(line);
      } catch (error) {
        throw new Error(`Invalid JSONL at line ${lineIndex + 1}: ${error.message}`);
      }
    });
}

function selectRecords(records, context) {
  if (context === "all") return records;
  if (context !== "latest") {
    return records.filter((record) => record.context === context);
  }

  const latest = records.reduce((winner, record) => {
    if (!record.context) return winner;
    if (!winner || (record.timestampMs ?? 0) > (winner.timestampMs ?? 0)) {
      return record;
    }
    return winner;
  }, null);
  if (!latest) return records;
  return records.filter((record) => record.context === latest.context);
}

function summarizeContexts(records) {
  const counts = new Map();
  for (const record of records) {
    counts.set(record.context ?? "(none)", (counts.get(record.context ?? "(none)") ?? 0) + 1);
  }
  return Object.fromEntries(counts);
}

function summarizeAppRecords(records) {
  const timings = records.map((record) => parseRawTiming(record)).filter(Boolean);
  const firstEnd = Math.min(...records.map((record) => record.timestampMs));
  const lastEnd = Math.max(...records.map((record) => record.timestampMs));
  const estimatedFirstStart = Math.min(
    ...timings.map((timing) => timing.endMs - timing.totalModelMs),
  );
  const estimatedWallMs =
    timings.length > 0 ? lastEnd - estimatedFirstStart : lastEnd - firstEnd;

  return {
    kind: "app-log",
    requestCount: records.length,
    okCount: records.filter((record) => record.ok).length,
    failedCount: records.filter((record) => !record.ok).length,
    inputChars: records.reduce((sum, record) => sum + record.inputs.join("").length, 0),
    outputChars: records.reduce((sum, record) => sum + (record.outputs ?? []).join("").length, 0),
    firstCompletionTimestampMs: firstEnd,
    lastCompletionTimestampMs: lastEnd,
    completionSpanMs: lastEnd - firstEnd,
    estimatedWallMs,
    promptTokens: sum(timings.map((timing) => timing.promptTokens)),
    predictedTokens: sum(timings.map((timing) => timing.predictedTokens)),
    totalTokens: sum(
      timings.map((timing) => timing.promptTokens + timing.predictedTokens),
    ),
    promptMs: sum(timings.map((timing) => timing.promptMs)),
    predictedMs: sum(timings.map((timing) => timing.predictedMs)),
    perRequestLatencyMs: percentileSummary(
      timings.map((timing) => timing.totalModelMs),
    ),
    predictedTokensPerSecondAggregate: rate(
      sum(timings.map((timing) => timing.predictedTokens)),
      estimatedWallMs,
    ),
    totalTokensPerSecondAggregate: rate(
      sum(timings.map((timing) => timing.promptTokens + timing.predictedTokens)),
      estimatedWallMs,
    ),
  };
}

function parseRawTiming(record) {
  if (!record.rawResponse) return null;
  try {
    const parsed = JSON.parse(record.rawResponse);
    const timings = parsed.timings ?? {};
    const promptMs = Number(timings.prompt_ms ?? 0);
    const predictedMs = Number(timings.predicted_ms ?? 0);
    return {
      endMs: Number(record.timestampMs),
      promptTokens: Number(timings.prompt_n ?? parsed.tokens_evaluated ?? 0),
      predictedTokens: Number(timings.predicted_n ?? parsed.tokens_predicted ?? 0),
      promptMs,
      predictedMs,
      totalModelMs: promptMs + predictedMs,
    };
  } catch {
    return null;
  }
}

function promptFromRecord(record, targetLang) {
  if (record.rawResponse) {
    try {
      const parsed = JSON.parse(record.rawResponse);
      if (typeof parsed.prompt === "string" && parsed.prompt.trim().length > 0) {
        return parsed.prompt;
      }
    } catch {
      // Fall through to reconstructing the app prompt.
    }
  }

  const sourceText = record.inputs[0] ?? "";
  return `${sourceLabel(record.sourceLang)}: ${cleanTextForRwkv(sourceText)}\n\n${targetLabel(
    targetLang,
  )}:`;
}

async function runBenchmark(records, options) {
  const results = [];
  const startedAt = performance.now();

  for (let offset = 0; offset < records.length; offset += options.concurrency) {
    const wave = records.slice(offset, offset + options.concurrency);
    const waveStartedAt = performance.now();
    const waveResults = await Promise.all(
      wave.map((record, index) =>
        translateOne(record, {
          ...options,
          globalIndex: offset + index,
          waveIndex: Math.floor(offset / options.concurrency),
          waveStartedAt,
        }),
      ),
    );
    results.push(...waveResults);
    const done = Math.min(offset + options.concurrency, records.length);
    console.log(
      `  ${done}/${records.length} done, last wave ${(performance.now() - waveStartedAt).toFixed(0)} ms`,
    );
  }

  const wallMs = performance.now() - startedAt;
  return {
    summary: summarizeBareResults(results, wallMs),
    results,
  };
}

async function translateOne(record, options) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), options.timeoutMs);
  const sourceText = record.inputs[0] ?? "";
  const body = {
    prompt: promptFromRecord(record, options.targetLang),
    n_predict: MAX_TOKENS_PER_SEGMENT,
    temperature: 1.0,
    stream: false,
  };
  const startedAt = performance.now();

  try {
    const response = await fetch(options.url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
    const responseText = await response.text();
    const endedAt = performance.now();
    let parsed = null;
    try {
      parsed = JSON.parse(responseText);
    } catch {
      // Keep the raw response preview below; the summary will count this as an error.
    }
    return {
      index: options.globalIndex,
      waveIndex: options.waveIndex,
      ok: response.ok && typeof parsed?.content === "string" && parsed.content.trim().length > 0,
      status: response.status,
      latencyMs: endedAt - startedAt,
      inputChars: sourceText.length,
      outputChars: typeof parsed?.content === "string" ? parsed.content.trim().length : 0,
      promptTokens: Number(parsed?.timings?.prompt_n ?? parsed?.tokens_evaluated ?? 0),
      predictedTokens: Number(parsed?.timings?.predicted_n ?? parsed?.tokens_predicted ?? 0),
      promptMs: Number(parsed?.timings?.prompt_ms ?? 0),
      predictedMs: Number(parsed?.timings?.predicted_ms ?? 0),
      stopType: parsed?.stop_type ?? null,
      truncated: Boolean(parsed?.truncated ?? false),
      error: response.ok ? null : responseText.slice(0, 500),
    };
  } catch (error) {
    return {
      index: options.globalIndex,
      waveIndex: options.waveIndex,
      ok: false,
      status: 0,
      latencyMs: performance.now() - startedAt,
      inputChars: sourceText.length,
      outputChars: 0,
      promptTokens: 0,
      predictedTokens: 0,
      promptMs: 0,
      predictedMs: 0,
      stopType: null,
      truncated: false,
      error: error.message,
    };
  } finally {
    clearTimeout(timeout);
  }
}

function summarizeBareResults(results, wallMs) {
  const okResults = results.filter((result) => result.ok);
  const promptTokens = sum(okResults.map((result) => result.promptTokens));
  const predictedTokens = sum(okResults.map((result) => result.predictedTokens));
  return {
    kind: "bare-replay",
    requestCount: results.length,
    okCount: okResults.length,
    failedCount: results.length - okResults.length,
    wallMs,
    inputChars: sum(results.map((result) => result.inputChars)),
    outputChars: sum(okResults.map((result) => result.outputChars)),
    promptTokens,
    predictedTokens,
    totalTokens: promptTokens + predictedTokens,
    promptMs: sum(okResults.map((result) => result.promptMs)),
    predictedMs: sum(okResults.map((result) => result.predictedMs)),
    truncatedCount: okResults.filter((result) => result.truncated).length,
    perRequestLatencyMs: percentileSummary(results.map((result) => result.latencyMs)),
    predictedTokensPerSecondAggregate: rate(predictedTokens, wallMs),
    totalTokensPerSecondAggregate: rate(promptTokens + predictedTokens, wallMs),
  };
}

function sourceLabel(sourceLang) {
  return roleLabelForLang(sourceLang || "en", "English");
}

function targetLabel(targetLang) {
  return roleLabelForLang(targetLang, "Chinese");
}

function roleLabelForLang(lang, fallback) {
  switch (lang) {
    case "en":
      return "English";
    case "zh-CN":
    case "zh-TW":
    case "zh":
      return "Chinese";
    case "ja":
      return "Japanese";
    case "ko":
      return "Korean";
    case "fr":
      return "French";
    case "de":
      return "German";
    case "es":
      return "Spanish";
    case "ru":
      return "Russian";
    case "pt":
      return "Portuguese";
    case "it":
      return "Italian";
    case "vi":
      return "Vietnamese";
    case "id":
      return "Indonesian";
    default:
      return fallback;
  }
}

function cleanTextForRwkv(input) {
  if (!hasRepeatedPunctuation(input)) return input;
  return collapseRepeatedPunctuation(input);
}

function hasRepeatedPunctuation(input) {
  let pendingPunctuation = null;
  let pendingWhitespace = false;
  for (const ch of input) {
    if (ch === pendingPunctuation && isRepeatablePunctuation(ch)) return true;
    if (/\s/u.test(ch)) {
      if (pendingPunctuation !== null) pendingWhitespace = true;
      continue;
    }
    if (pendingWhitespace && ch === pendingPunctuation) return true;
    pendingPunctuation = isRepeatablePunctuation(ch) ? ch : null;
    pendingWhitespace = false;
  }
  return false;
}

function collapseRepeatedPunctuation(input) {
  let output = "";
  let lastPunctuation = null;
  let pendingSpaces = "";
  for (const ch of input) {
    if (/\s/u.test(ch)) {
      if (lastPunctuation !== null) pendingSpaces += ch;
      else output += ch;
      continue;
    }
    if (ch === lastPunctuation) {
      pendingSpaces = "";
      continue;
    }
    output += pendingSpaces;
    pendingSpaces = "";
    output += ch;
    lastPunctuation = isRepeatablePunctuation(ch) ? ch : null;
  }
  return output + pendingSpaces;
}

function isRepeatablePunctuation(ch) {
  return ".,!?,;:。？！；：，、".includes(ch);
}

function printSummary(title, summary) {
  const wallMs = summary.wallMs ?? summary.estimatedWallMs;
  console.log(`\n${title}`);
  console.log(`  requests: ${summary.requestCount}, ok: ${summary.okCount}, failed: ${summary.failedCount}`);
  console.log(`  wall: ${formatMs(wallMs)}`);
  console.log(
    `  tokens: prompt=${summary.promptTokens}, predicted=${summary.predictedTokens}, total=${summary.totalTokens}`,
  );
  console.log(
    `  throughput: predicted=${summary.predictedTokensPerSecondAggregate.toFixed(
      2,
    )} tok/s, total=${summary.totalTokensPerSecondAggregate.toFixed(2)} tok/s`,
  );
  console.log(
    `  latency p50/p90/p99: ${formatMs(summary.perRequestLatencyMs.p50)} / ${formatMs(
      summary.perRequestLatencyMs.p90,
    )} / ${formatMs(summary.perRequestLatencyMs.p99)}`,
  );
}

function printComparison(appBaseline, bareSummary) {
  const appWall = appBaseline.estimatedWallMs;
  const bareWall = bareSummary.wallMs;
  console.log("\nComparison");
  console.log(`  wall ratio bare/app: ${(bareWall / appWall).toFixed(3)}x`);
  console.log(
    `  predicted tok/s ratio bare/app: ${(
      bareSummary.predictedTokensPerSecondAggregate /
      appBaseline.predictedTokensPerSecondAggregate
    ).toFixed(3)}x`,
  );
}

function percentileSummary(values) {
  if (values.length === 0) return { min: 0, p50: 0, p90: 0, p99: 0, max: 0, avg: 0 };
  const sorted = [...values].sort((a, b) => a - b);
  return {
    min: sorted[0],
    p50: percentile(sorted, 0.5),
    p90: percentile(sorted, 0.9),
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
  if (!Number.isFinite(ms)) return "n/a";
  return `${ms.toFixed(0)} ms`;
}

main().catch((error) => {
  console.error(error.stack || error.message);
  process.exitCode = 1;
});
