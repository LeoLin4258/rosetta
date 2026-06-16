import { useEffect, useState } from "react";
import { ArrowRight, Check } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type DoneStepProps = {
  /** "local" = downloaded local engine. "external" = chose external API. */
  variant: "local" | "local-pdf-skipped" | "external";
  onContinue: () => void;
  isContinuing: boolean;
};

/**
 * Final screen of Onboarding. Big checkmark with a small scale-in animation
 * (CSS-only — no framer-motion in the project), one CTA to enter Workspace.
 *
 * "Product感优先" applied: smooth reveal of the success state instead of a
 * jarring instant-flip from progress bar to button.
 */
export function DoneStep({ variant, onContinue, isContinuing }: DoneStepProps) {
  const [revealed, setRevealed] = useState(false);
  useEffect(() => {
    // Microtask-delayed mount so the CSS transition fires from the
    // initial (scale-75 + opacity-0) state.
    const id = window.setTimeout(() => setRevealed(true), 40);
    return () => window.clearTimeout(id);
  }, []);

  const heading = variant === "local" ? "本地翻译已就绪" : "好的";
  const subheading =
    variant === "local"
      ? "翻译模型和 PDF 组件都已准备好。开始你的第一个文档翻译吧。"
      : variant === "local-pdf-skipped"
        ? "翻译模型已准备好。PDF 组件可在设置中安装。"
        : "你可以在 设置 → 外部翻译 API 中填入自己的端点。";
  const cta = variant === "external" ? "进入 Rosetta" : "翻译我的第一个文档";

  return (
    <div className="flex h-full flex-col items-center justify-between gap-4 px-14 py-10">
      <div className="flex flex-1 flex-col items-center justify-center gap-6 text-center">
        <div
          className={cn(
            "flex size-20 items-center justify-center rounded-full bg-emerald-500/15 text-emerald-500 transition-all duration-500 ease-out",
            revealed ? "scale-100 opacity-100" : "scale-75 opacity-0"
          )}
        >
          <Check className="size-10" strokeWidth={2.5} />
        </div>
        <div
          className={cn(
            "space-y-2 transition-all duration-500 delay-150 ease-out",
            revealed ? "translate-y-0 opacity-100" : "translate-y-2 opacity-0"
          )}
        >
          <h2 className="text-2xl font-semibold tracking-tight">{heading}</h2>
          <p className="max-w-md text-sm leading-relaxed text-muted-foreground">
            {subheading}
          </p>
        </div>
        <div
          className={cn(
            "transition-all duration-500 delay-300 ease-out",
            revealed ? "translate-y-0 opacity-100" : "translate-y-2 opacity-0"
          )}
        >
          <Button
            size="lg"
            onClick={onContinue}
            disabled={isContinuing}
            className="min-w-44 gap-2"
          >
            {cta}
            <ArrowRight className="size-4" />
          </Button>
        </div>
      </div>
    </div>
  );
}
