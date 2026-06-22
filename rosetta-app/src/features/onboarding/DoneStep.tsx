import { ArrowRight, Check } from "lucide-react";

import { Button } from "@/components/ui/button";

import { OnboardingStepShell } from "./OnboardingStepShell";

type DoneStepProps = {
  skippedLocalInstall: boolean;
  skippedPdfInstall: boolean;
  onContinue: () => void;
  isContinuing: boolean;
};

/**
 * Final step of the guided setup. Kept intentionally plain so the only strong
 * action left is entering the workspace.
 */
export function DoneStep({
  skippedLocalInstall,
  skippedPdfInstall,
  onContinue,
  isContinuing,
}: DoneStepProps) {
  const heading = "欢迎使用 Rosetta";
  const subheading = skippedLocalInstall
    ? skippedPdfInstall
      ? "你可以先开始使用 Rosetta，之后再在设置中配置外部翻译 API，并补装 PDF 组件。"
      : "PDF 组件已经准备好。开始使用后，可在设置中配置你的外部翻译 API。"
    : skippedPdfInstall
      ? "本地翻译引擎已经准备好。PDF 组件可在设置中随时补装。"
      : "本地翻译引擎和 PDF 组件都已准备好，可以开始你的第一个文档翻译。";

  return (
    <OnboardingStepShell
      stepLabel="步骤 3 / 3"
      progressValue={100}
      title={heading}
      description={subheading}
      align="start"
    >
      <div className="flex items-center gap-2 text-emerald-600">
        <div className="flex size-6 items-center justify-center rounded-full bg-emerald-500/12">
          <Check className="size-4" strokeWidth={2.2} />
        </div>
        <span className="text-xs font-medium">已完成准备</span>
      </div>

      <Button
        size="lg"
        onClick={onContinue}
        disabled={isContinuing}
        className="h-11 w-full gap-2"
      >
        开始使用
        <ArrowRight className="size-4" />
      </Button>
    </OnboardingStepShell>
  );
}
