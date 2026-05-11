import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

import type { RosettaTranslationFile } from "../types/rosetta";

export async function openSourcePreviewWindow({
  jobId,
  sourceFileId,
  sourceFilename,
}: {
  jobId: string;
  sourceFileId: string;
  sourceFilename: string;
}) {
  const url = `/#/preview/${encodeURIComponent(jobId)}/sources/${encodeURIComponent(
    sourceFileId
  )}`;
  const label = `source-${safeWindowLabel(jobId)}-${safeWindowLabel(sourceFileId)}`;

  await openPreviewWindow({
    height: 760,
    label,
    minHeight: 600,
    minWidth: 760,
    title: sourceFilename,
    url,
    width: 900,
  });
}

export async function openTranslationPreviewWindow({
  jobId,
  sourceFilename,
  translationFile,
}: {
  jobId: string;
  sourceFilename: string;
  translationFile: RosettaTranslationFile;
}) {
  const url = `/#/preview/${encodeURIComponent(jobId)}/translations/${encodeURIComponent(
    translationFile.id
  )}`;
  const label = `translation-${safeWindowLabel(translationFile.id)}`;
  const title = `${sourceFilename} · ${translationFile.targetLang}`;

  await openPreviewWindow({
    height: 760,
    label,
    minHeight: 600,
    minWidth: 900,
    title,
    url,
    width: 1180,
  });
}

async function openPreviewWindow({
  height,
  label,
  minHeight,
  minWidth,
  title,
  url,
  width,
}: {
  height: number;
  label: string;
  minHeight: number;
  minWidth: number;
  title: string;
  url: string;
  width: number;
}) {
  if (!("__TAURI_INTERNALS__" in window)) {
    window.open(url, label);
    return;
  }

  const existing = await WebviewWindow.getByLabel(label);
  if (existing) {
    await existing.show();
    await existing.setFocus();
    return;
  }

  new WebviewWindow(label, {
    url,
    title,
    width,
    height,
    minWidth,
    minHeight,
    decorations: true,
    shadow: true,
  });
}

function safeWindowLabel(value: string) {
  return value.replace(/[^a-zA-Z0-9\-_:]/g, "_");
}
