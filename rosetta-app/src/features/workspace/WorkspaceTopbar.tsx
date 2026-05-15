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
  { value: "en", label: "English" },
];

const SOURCE_LANGS = [
  { value: "auto", label: "自动检测" },
  { value: "zh-CN", label: "中文" },
  { value: "en", label: "English" },
];

type WorkspaceTopbarProps = {
  job: RosettaJobSummary;
  activeTranslationFile: RosettaTranslationFile | null;
  isTranslating: boolean;
  isRuntimeStarting: boolean;
  translatedCount: number;
  totalCount: number;
  sourceLang: string;
  targetLang: string;
  selectedBlockCount: number;
  onSourceLangChange: (lang: string) => void;
  onTargetLangChange: (lang: string) => void;
  onTranslate: (targetLang: string, sourceLang: string) => void;
  onCancelTranslation: () => void;
  onExport: (kind: "translation" | "bilingual") => void;
  onRetranslateSelected: () => void;
};

export function WorkspaceTopbar({
  job,
  activeTranslationFile,
  isTranslating,
  isRuntimeStarting,
  translatedCount,
  totalCount,
  sourceLang,
  targetLang,
  selectedBlockCount,
  onSourceLangChange,
  onTargetLangChange,
  onTranslate,
  onCancelTranslation,
  onExport,
  onRetranslateSelected,
}: WorkspaceTopbarProps) {
  const [confirmingCancel, setConfirmingCancel] = useState(false);

  const hasTranslation =
    activeTranslationFile && activeTranslationFile.completedSegments > 0;
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
              {translatedCount} / {totalCount} · {progressPercent}%
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
            {selectedBlockCount > 0 && (
              <Button
                size="sm"
                variant="outline"
                onClick={onRetranslateSelected}
                className="gap-1.5"
              >
                <RefreshCw className="size-3.5" />
                重翻选中 {selectedBlockCount} 段
              </Button>
            )}

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

            {isRuntimeStarting ? (
              <Button size="sm" disabled className="gap-1.5">
                <Loader2 className="size-3.5 animate-spin" />
                正在启动模型…
              </Button>
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
