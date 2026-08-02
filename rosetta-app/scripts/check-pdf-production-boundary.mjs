import assert from "node:assert/strict";
import { access, readFile, readdir } from "node:fs/promises";

const v3Commands = [
  "probe_rosetta_pdf_v3_component",
  "list_rosetta_pdf_v3_runs",
  "create_rosetta_pdf_v3_run",
  "cancel_rosetta_pdf_v3_run",
  "get_rosetta_pdf_v3_run_status",
  "pause_rosetta_pdf_v3_run",
  "recover_rosetta_pdf_v3_run",
  "render_rosetta_pdf_v3_translated_page_as_png",
  "retry_rosetta_pdf_v3_page",
  "resume_rosetta_pdf_v3_run",
  "export_rosetta_pdf_v3_run",
];

const cargo = await read("src-tauri/Cargo.toml");
const rustSources = await readTree("src-tauri/src", ".rs");
const frontendCommands = await read("src/lib/rosettaJobs.ts");
const frontendTypes = await read("src/types/rosetta.ts");
const sourceState = await read("src-tauri/src/rosetta_jobs/formats/pdf/source_state.rs");

assert.doesNotMatch(cargo, /experimental-pdf-v3/u);
for (const dependency of ["pdf", "memmap2", "subsetter", "ttf-parser"]) {
  assert.doesNotMatch(
    cargo,
    new RegExp(`^${dependency}\\s*=`, "mu"),
    `${dependency} must not return as an unused native PDF v3 dependency`,
  );
}
assert.match(cargo, /^pdfium-render\s*=/mu);
assert.match(cargo, /^lopdf\s*=/mu);

await assertMissing("src-tauri/src/pdf_v3/mod.rs");
for (const source of rustSources) {
  assert.doesNotMatch(source.text, /experimental-pdf-v3|pdf_v3|PdfV3/u, source.path);
}
for (const command of v3Commands) {
  assert.doesNotMatch(rustSources.map(({ text }) => text).join("\n"), new RegExp(command, "u"));
  assert.doesNotMatch(frontendCommands, new RegExp(command, "u"));
}

assert.doesNotMatch(frontendCommands, /PdfV3|pdfV3|rosetta_pdf_v3/u);
assert.doesNotMatch(frontendTypes, /PdfV3/u);
await assertMissing("src/features/workspace/usePdfV3RunControl.ts");
await assertMissing("src/features/preview/usePdfV3Preview.ts");

for (const productionWrapper of [
  "preparseRosettaPdfPages",
  "translateRosettaPdfPages",
  "exportRosettaTranslatedPdf",
  "renderRosettaPdfPageAsPng",
  "renderRosettaPdfTranslatedPageAsPng",
]) {
  assert.match(frontendCommands, new RegExp(`function ${productionWrapper}\\b`, "u"));
}
assert.match(sourceState, /Sha256::new\(\)/u);
assert.match(sourceState, /sha256:\{:x\}/u);

console.log("PDF production boundary checks passed");

function read(path) {
  return readFile(new URL(`../${path}`, import.meta.url), "utf8");
}

async function readTree(path, extension) {
  const root = new URL(`../${path}/`, import.meta.url);
  const files = [];
  await visit(root);
  return files;

  async function visit(directory) {
    for (const entry of await readdir(directory, { withFileTypes: true })) {
      const child = new URL(`${entry.name}${entry.isDirectory() ? "/" : ""}`, directory);
      if (entry.isDirectory()) {
        await visit(child);
      } else if (entry.name.endsWith(extension)) {
        files.push({ path: child.pathname, text: await readFile(child, "utf8") });
      }
    }
  }
}

async function assertMissing(path) {
  await assert.rejects(access(new URL(`../${path}`, import.meta.url)), { code: "ENOENT" });
}
