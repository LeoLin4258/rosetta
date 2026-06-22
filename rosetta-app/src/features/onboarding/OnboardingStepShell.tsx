import type { ReactNode } from "react";

import { cn } from "@/lib/utils";

type OnboardingStepShellProps = {
  stepLabel: string;
  title: string;
  description?: string;
  progressValue: number;
  children: ReactNode;
  contentClassName?: string;
  align?: "start" | "center";
};

export function OnboardingStepShell({
  stepLabel,
  title,
  description,
  progressValue,
  children,
  contentClassName,
  align = "center",
}: OnboardingStepShellProps) {
  return (
    <div className="flex h-full items-center justify-center px-8 py-14">
      <div className="w-full max-w-[28rem] space-y-12">
        <div className="space-y-6">
          <div className="space-y-3">
            <p className="text-xs font-medium text-muted-foreground">{stepLabel}</p>
            <div
              className="h-1 w-full overflow-hidden rounded-full bg-muted"
              aria-hidden="true"
            >
              <div
                className="h-full rounded-full bg-foreground/80 transition-[width] duration-200"
                style={{ width: `${progressValue}%` }}
              />
            </div>
          </div>
          <div className="space-y-4">
            <h1 className="text-3xl font-semibold tracking-tight text-foreground">
              {title}
            </h1>
            {description ? (
              <p className=" text-sm leading-6 text-muted-foreground">
                {description}
              </p>
            ) : null}
          </div>
        </div>

        <div
          className={cn(
            "flex flex-col gap-6",
            align === "center" ? "items-center text-center" : "items-stretch text-left",
            contentClassName
          )}
        >
          {children}
        </div>
      </div>
    </div>
  );
}
