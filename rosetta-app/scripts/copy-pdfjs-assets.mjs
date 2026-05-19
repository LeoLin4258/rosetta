#!/usr/bin/env node
/**
 * Stage pdfjs-dist's worker bundle and CJK CMap pack into `public/pdfjs/` so
 * Vite serves them as static assets the running webview can fetch.
 *
 * Why a postinstall script instead of vite imports: the worker is loaded as
 * `new Worker(workerUrl)` from inside react-pdf, so we need a stable URL
 * (`/pdfjs/pdf.worker.min.mjs`) rather than letting Vite hash-rename it. Same
 * deal for cmaps, which pdfjs fetches lazily via XHR when a PDF references
 * a CJK font without embedding it.
 *
 * Re-runs are cheap: we just rsync-style overwrite. The destination is in
 * .gitignore so it doesn't pollute the repo.
 */
import { existsSync, mkdirSync, cpSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..");
const src = resolve(repoRoot, "node_modules/pdfjs-dist");
const dst = resolve(repoRoot, "public/pdfjs");

if (!existsSync(src)) {
  // Likely an install in progress; bail quietly so we don't error during
  // `pnpm install` itself (which runs postinstall before node_modules is
  // fully linked in some pnpm modes).
  console.warn(
    `[pdfjs] node_modules/pdfjs-dist not found yet; skipping. Re-run \`pnpm postinstall\` if needed.`,
  );
  process.exit(0);
}

mkdirSync(dst, { recursive: true });

const copy = (relSrc, relDst) => {
  const from = join(src, relSrc);
  const to = join(dst, relDst);
  if (!existsSync(from)) {
    console.warn(`[pdfjs] missing ${relSrc}, skipped`);
    return;
  }
  mkdirSync(dirname(to), { recursive: true });
  cpSync(from, to, { recursive: true });
  const size = statSync(to).size ?? "?";
  console.log(`[pdfjs] ${relSrc} → public/pdfjs/${relDst}${typeof size === "number" ? ` (${size} bytes)` : ""}`);
};

copy("build/pdf.worker.min.mjs", "pdf.worker.min.mjs");
copy("cmaps", "cmaps");
// Standard14 fonts that pdfjs falls back to when a source PDF references a
// non-embedded base font. Cheap (~190 KB) and avoids "PDF text shows up as
// boxes" surprises on stripped-down PDFs.
copy("standard_fonts", "standard_fonts");

console.log("[pdfjs] assets staged.");
