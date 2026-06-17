import { FileText } from "lucide-react";

import { Button } from "@/components/ui/button";

type PdfSetupStepProps = {
  onBeginInstall: () => void;
  onSkip: () => void;
  isInstalling: boolean;
};

export function PdfSetupStep({
  onBeginInstall,
  onSkip,
  isInstalling,
}: PdfSetupStepProps) {
  return (
    <div className="flex h-full flex-col items-center justify-between gap-4 px-14 py-10">
      <div className="flex w-full flex-1 flex-col items-center justify-center gap-5 text-center">
        <div className="flex size-14 items-center justify-center rounded-2xl bg-primary/10 text-primary">
          <FileText className="size-7" strokeWidth={1.5} />
        </div>
        <div className="space-y-2">
          <h2 className="text-xl font-semibold">准备 PDF 版面处理</h2>
          <p className="max-w-md text-sm leading-relaxed text-muted-foreground">
            Rosetta 可以保留 PDF 的原始排版，并生成可预览、可导出的译文 PDF。
          </p>
        </div>
        <div className="flex flex-col items-center gap-3">
          <Button
            size="lg"
            onClick={onBeginInstall}
            disabled={isInstalling}
            className="min-w-48"
          >
            安装 PDF 版面处理组件
          </Button>
          <button
            type="button"
            onClick={onSkip}
            className="text-xs text-muted-foreground/35 transition-colors hover:text-muted-foreground/60"
          >
            稍后再装
          </button>
        </div>
      </div>
    </div>
  );
}
