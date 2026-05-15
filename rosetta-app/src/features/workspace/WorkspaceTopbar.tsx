import { useState } from "react";
import { Download, Play, Square } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useRosettaStore } from "@/store/useRosettaStore";
import type { RosettaJobSummary, RosettaTranslationFile } from "@/types/rosetta";

const TARGET_LANGS = [
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
];

type WorkspaceTopbarProps = {
  job: RosettaJobSummary;
  activeTranslationFile: RosettaTranslationFile | null;
  isTranslating: boolean;
  translatedCount: number;
  totalCount: number;
  onTranslate: (targetLang: string) => void;
  onCancelTranslation: () => void;
  onExport: (kind: "translation" | "bilingual") => void;
};

export function WorkspaceTopbar({
  job,
  activeTranslationFile,
  isTranslating,
  translatedCount,
  totalCount,
  onTranslate,
  onCancelTranslation,
  onExport,
}: WorkspaceTopbarProps) {
  const defaultTargetLang = useRosettaStore((s) => s.defaultTargetLang);
  const setDefaultTargetLang = useRosettaStore((s) => s.setDefaultTargetLang);
  const [confirmingCancel, setConfirmingCancel] = useState(false);

  const hasTranslation = activeTranslationFile && activeTranslationFile.completedSegments > 0;
  const progressPercent = totalCount > 0 ? Math.round((translatedCount / totalCount) * 100) : 0;

  function handleCancel() {
    if (confirmingCancel) {
      onCancelTranslation();
      setConfirmingCancel(false);
    } else {
      setConfirmingCancel(true);
    }
  }

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
            <span className="text-xs text-muted-foreground/60 tabular-nums">
              {translatedCount} / {totalCount} · {progressPercent}%
            </span>
            {confirmingCancel ? (
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground/60">确认取消？</span>
                <button
                  type="button"
                  onClick={handleCancel}
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
              <Button size="sm" variant="outline" onClick={() => setConfirmingCancel(true)} className="gap-1.5">
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
            <Select value={defaultTargetLang} onValueChange={setDefaultTargetLang}>
              <SelectTrigger className="h-8 w-28 text-xs">
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
            <Button
              size="sm"
              onClick={() => onTranslate(defaultTargetLang)}
              className="gap-1.5"
            >
              <Play className="size-3.5" /> 翻译
            </Button>
          </>
        )}
      </div>
    </div>
  );
}
