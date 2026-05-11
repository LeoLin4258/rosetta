import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

import type { RosettaTranslationFile } from "../types/rosetta";

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
    width: 1180,
    height: 760,
    minWidth: 900,
    minHeight: 600,
    decorations: true,
    shadow: true,
  });
}

function safeWindowLabel(value: string) {
  return value.replace(/[^a-zA-Z0-9\-_:]/g, "_");
}
