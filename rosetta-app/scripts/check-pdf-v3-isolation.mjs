import assert from "node:assert/strict";
import { access, readFile } from "node:fs/promises";

const feature = "experimental-pdf-v3";
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
const tauriLib = await read("src-tauri/src/lib.rs");
const pdfModules = await read("src-tauri/src/rosetta_jobs/formats/pdf/mod.rs");
const nativePdfV3 = await read("src-tauri/src/pdf_v3/mod.rs");
const frontendCommands = await read("src/lib/rosettaJobs.ts");
const frontendTypes = await read("src/types/rosetta.ts");
const sourceState = await read("src-tauri/src/rosetta_jobs/formats/pdf/source_state.rs");

assert.match(cargo, /\[features\]\s+default = \[\]\s+experimental-pdf-v3 = \[\]/u);
assert.match(cargo, /pdfium-render = /u);
assert.match(
  nativePdfV3,
  /#!\[cfg_attr\(any\(feature = "experimental-pdf-v3", test\), allow\(dead_code\)\)\]/u,
);
assert.doesNotMatch(nativePdfV3, /#!\[allow\(dead_code\)\]/u);

for (const command of v3Commands) {
  assert.match(
    tauriLib,
    new RegExp(
      `#\\[cfg\\(feature = "${feature}"\\)\\]\\s+rosetta_jobs::${command}`,
      "u",
    ),
    `${command} must only be registered by the experimental feature`,
  );
  assert.doesNotMatch(
    frontendCommands,
    new RegExp(command, "u"),
    `${command} must not have a production frontend wrapper`,
  );
}

for (const moduleName of [
  "v3_component",
  "v3_control",
  "v3_export",
  "v3_lifecycle",
  "v3_preview",
  "v3_processor",
  "v3_run_creation",
  "v3_run_list",
  "v3_runtime",
  "v3_source_identity",
  "v3_worker",
]) {
  assert.match(
    pdfModules,
    new RegExp(
      `#\\[cfg\\(feature = "${feature}"\\)\\]\\s+pub\\(crate\\) mod ${moduleName};`,
      "u",
    ),
    `${moduleName} must only compile with the experimental feature`,
  );
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
assert.match(sourceState, /pdf_v3::document::DocumentHandle/u);

console.log("PDF v3 isolation boundary checks passed");

function read(path) {
  return readFile(new URL(`../${path}`, import.meta.url), "utf8");
}

async function assertMissing(path) {
  await assert.rejects(access(new URL(`../${path}`, import.meta.url)), { code: "ENOENT" });
}
