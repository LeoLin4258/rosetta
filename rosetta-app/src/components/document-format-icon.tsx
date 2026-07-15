import markdownIconUrl from "@/assets/icons/markdown.svg";
import pdfIconUrl from "@/assets/icons/pdf.svg";
import textIconUrl from "@/assets/icons/text.svg";
import { cn } from "@/lib/utils";
import type { RosettaSourceDocumentFormat } from "@/types/rosetta";

type DocumentFormatIconProps = {
  format: RosettaSourceDocumentFormat;
  isPdfPrepared?: boolean;
  className?: string;
};

export function DocumentFormatIcon({
  format,
  isPdfPrepared = false,
  className,
}: DocumentFormatIconProps) {
  const iconUrl =
    format === "pdf"
      ? pdfIconUrl
      : format === "markdown"
        ? markdownIconUrl
        : textIconUrl;

  return (
    <span
      className={cn(
        "relative inline-flex size-4 shrink-0 items-center justify-center",
        className
      )}
      title={isPdfPrepared ? "PDF 已预解析" : undefined}
    >
      <img
        src={iconUrl}
        alt=""
        className="size-4 opacity-70 dark:invert"
        aria-hidden="true"
      />
      {isPdfPrepared ? (
        <span
          className="absolute -right-px -bottom-px size-1.5 rounded-full bg-emerald-600 ring-[1.5px] ring-sidebar dark:bg-emerald-400"
          aria-hidden="true"
        />
      ) : null}
      {isPdfPrepared ? <span className="sr-only">PDF 已预解析</span> : null}
    </span>
  );
}
