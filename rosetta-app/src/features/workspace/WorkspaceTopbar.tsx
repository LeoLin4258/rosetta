import { useState } from "react";
import { Download, Loader2, Play, RefreshCw, Square } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { RosettaJobSummary, RosettaTranslationFile } from "@/types/rosetta";

const TARGET_LANGS = [
  { value: "zh-CN", label: "简体中文" },
  { value: "en", label: "英文" },
];

const SOURCE_LANGS = [
  { value: "auto", label: "自动检测" },
  { value: "zh-CN", label: "简体中文" },
  { value: "en", label: "英文" },
];

type WorkspaceTopbarProps = {
  job: RosettaJobSummary;
  activeTranslationFile: RosettaTranslationFile | null;
  isTranslating: boolean;
  isRuntimeStarting: boolean;
  isPdfEngineInstalling?: boolean;
  pdfEngineProgressMessage?: string | null;
  translatedCount: number;
  totalCount: number;
  pdfProgress?: { phase: string; percent: number | null } | null;
  sourceLang: string;
  targetLang: string;
  selectedBlockCount: number;
  onSourceLangChange: (lang: string) => void;
  onTargetLangChange: (lang: string) => void;
  onTranslate: (targetLang: string, sourceLang: string) => void;
  onCancelTranslation: () => void;
  onExport: (kind: "translation" | "bilingual") => void;
  onRetranslateSelected: () => void;
  onRetranslateAll: () => void;
};

const PDF_PHASE_LABELS: Record<string, string> = {
  parse: "解析版面",
  translate: "翻译中",
  render: "生成 PDF",
};

export function WorkspaceTopbar({
  job,
  activeTranslationFile,
  isTranslating,
  isRuntimeStarting,
  isPdfEngineInstalling = false,
  pdfEngineProgressMessage = null,
  translatedCount,
  totalCount,
  pdfProgress = null,
  sourceLang,
  targetLang,
  selectedBlockCount,
  onSourceLangChange,
  onTargetLangChange,
  onTranslate,
  onCancelTranslation,
  onExport,
  onRetranslateSelected,
  onRetranslateAll,
}: WorkspaceTopbarProps) {
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  const [confirmingRetranslateAll, setConfirmingRetranslateAll] = useState(false);

  const hasTranslation =
    activeTranslationFile &&
    (job.format === "pdf" ||
      activeTranslationFile.completedSegments > 0);
  const allTranslated =
    !!activeTranslationFile &&
    (job.format === "pdf"
      ? activeTranslationFile.status === "translated"
      : activeTranslationFile.segmentCount > 0 &&
        activeTranslationFile.completedSegments >= activeTranslationFile.segmentCount);
  const progressPercent =
    totalCount > 0 ? Math.round((translatedCount / totalCount) * 100) : 0;

  return (
    <div className="flex items-center justify-between border-b border-border/40 px-6 py-2.5">
      {/* Left: doc name */}
      <div className="flex min-w-0 items-center gap-2">
        <span className="truncate text-sm font-medium">{job.filename}</span>
        <span className="shrink-0 rounded bg-muted/50 px-1.5 py-0.5 text-xs text-muted-foreground/60">
          {job.format}
        </span>
      </div>

      {/* Right: actions */}
      <div className="flex shrink-0 items-center gap-2">
        {isTranslating ? (
          <>
            <span className="text-xs tabular-nums text-muted-foreground/60">
              {pdfProgress != null ? (
                <>
                  {PDF_PHASE_LABELS[pdfProgress.phase] ?? pdfProgress.phase}
                  {pdfProgress.percent != null ? ` · ${pdfProgress.percent}%` : ""}
                </>
              ) : (
                <>{translatedCount} / {totalCount} · {progressPercent}%</>
              )}
            </span>
            {confirmingCancel ? (
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground/60">确认取消？</span>
                <button
                  type="button"
                  onClick={() => {
                    onCancelTranslation();
                    setConfirmingCancel(false);
                  }}
                  className="text-xs text-destructive/70 transition-colors hover:text-destructive"
                >
                  确定
                </button>
                <button
                  type="button"
                  onClick={() => setConfirmingCancel(false)}
                  className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground"
                >
                  继续
                </button>
              </div>
            ) : (
              <Button
                size="sm"
                variant="outline"
                onClick={() => setConfirmingCancel(true)}
                className="gap-1.5"
              >
                <Square className="size-3" /> 取消
              </Button>
            )}
          </>
        ) : (
          <>
            {hasTranslation && (
              <Button
                size="sm"
                variant="outline"
                onClick={() => onExport("translation")}
                className="gap-1.5"
              >
                <Download className="size-3.5" /> 导出译文
              </Button>
            )}

            {/* Source language */}
            <Select value={sourceLang} onValueChange={onSourceLangChange}>
              <SelectTrigger className="h-8 w-28 text-xs">
                <SelectValue placeholder="原文语言" />
              </SelectTrigger>
              <SelectContent>
                {SOURCE_LANGS.map((lang) => (
                  <SelectItem key={lang.value} value={lang.value} className="text-xs">
                    {lang.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {/* Target language + translate */}
            <Select value={targetLang} onValueChange={onTargetLangChange}>
              <SelectTrigger className="h-8 w-24 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {TARGET_LANGS.map((lang) => (
                  <SelectItem key={lang.value} value={lang.value} className="text-xs">
                    {lang.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>

            {isPdfEngineInstalling ? (
              <Button size="sm" disabled className="gap-1.5">
                <Loader2 className="size-3.5 animate-spin" />
                {pdfEngineProgressMessage ?? "正在准备 PDF 引擎…"}
              </Button>
            ) : isRuntimeStarting ? (
              <Button size="sm" disabled className="gap-1.5">
                <Loader2 className="size-3.5 animate-spin" />
                正在启动模型…
              </Button>
            ) : selectedBlockCount > 0 ? (
              <Button
                size="sm"
                disabled={sourceLang !== "auto" && sourceLang === targetLang}
                onClick={onRetranslateSelected}
                className="gap-1.5"
                title={sourceLang !== "auto" && sourceLang === targetLang ? "原文与译文语言不能相同" : undefined}
              >
                <RefreshCw className="size-3.5" />
                重翻选中 {selectedBlockCount} 段
              </Button>
            ) : allTranslated ? (
              confirmingRetranslateAll ? (
                <div className="flex items-center gap-2">
                  <span className="text-xs text-muted-foreground/60">确认重翻全部？</span>
                  <button
                    type="button"
                    onClick={() => {
                      onRetranslateAll();
                      setConfirmingRetranslateAll(false);
                    }}
                    className="text-xs text-destructive/70 transition-colors hover:text-destructive"
                  >
                    确定
                  </button>
                  <button
                    type="button"
                    onClick={() => setConfirmingRetranslateAll(false)}
                    className="text-xs text-muted-foreground/40 transition-colors hover:text-muted-foreground"
                  >
                    取消
                  </button>
                </div>
              ) : (
                <Button
                  size="sm"
                  disabled={sourceLang !== "auto" && sourceLang === targetLang}
                  onClick={() => setConfirmingRetranslateAll(true)}
                  className="gap-1.5"
                  title={sourceLang !== "auto" && sourceLang === targetLang ? "原文与译文语言不能相同" : undefined}
                >
                  <RefreshCw className="size-3.5" /> 重翻全部
                </Button>
              )
            ) : (
              <Button
                size="sm"
                disabled={sourceLang !== "auto" && sourceLang === targetLang}
                onClick={() => onTranslate(targetLang, sourceLang)}
                className="gap-1.5"
                title={sourceLang !== "auto" && sourceLang === targetLang ? "原文与译文语言不能相同" : undefined}
              >
                <Play className="size-3.5" /> 翻译
              </Button>
            )}
          </>
        )}
      </div>
    </div>
  );
}
