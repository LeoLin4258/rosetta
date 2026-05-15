import { useMemo } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { Components } from "react-markdown";
import type { RosettaSourceDocumentFormat, Segment, TranslationSegment } from "@/types/rosetta";

type BilingualViewProps = {
  segments: Segment[];
  translationSegments: TranslationSegment[];
  format: RosettaSourceDocumentFormat;
};

// ─── Markdown reconstruction ──────────────────────────────────────────────────

function segmentsToMarkdown(segments: Segment[], getText: (seg: Segment) => string): string {
  const lines: string[] = [];
  let i = 0;
  while (i < segments.length) {
    const seg = segments[i];

    if (seg.kind === "list_item") {
      let hasItems = false;
      while (i < segments.length && segments[i].kind === "list_item") {
        const itemText = getText(segments[i]);
        if (itemText) {
          lines.push(`- ${itemText}`);
          hasItems = true;
        }
        i++;
      }
      if (hasItems) lines.push("");
      continue;
    }

    const text = getText(seg);
    switch (seg.kind) {
      case "heading":
        if (text) lines.push(`## ${text}`, "");
        break;
      case "blockquote":
        if (text) lines.push(`> ${text}`, "");
        break;
      case "code":
        if (text) lines.push("```", text, "```", "");
        break;
      case "caption":
      case "footnote":
        if (text) lines.push(`*${text}*`, "");
        break;
      case "metadata":
        break; // skip metadata in rendered view
      default:
        if (text) lines.push(text, "");
        break;
    }
    i++;
  }
  return lines.join("\n");
}

// ─── Tailwind-styled markdown components ─────────────────────────────────────

const mdComponents: Components = {
  h1: ({ children }) => (
    <h1 className="mb-4 mt-6 text-xl font-bold first:mt-0">{children}</h1>
  ),
  h2: ({ children }) => (
    <h2 className="mb-3 mt-5 text-lg font-semibold first:mt-0">{children}</h2>
  ),
  h3: ({ children }) => (
    <h3 className="mb-2 mt-4 text-base font-semibold first:mt-0">{children}</h3>
  ),
  h4: ({ children }) => (
    <h4 className="mb-2 mt-3 text-sm font-semibold first:mt-0">{children}</h4>
  ),
  p: ({ children }) => (
    <p className="mb-3 text-sm leading-relaxed last:mb-0">{children}</p>
  ),
  ul: ({ children }) => (
    <ul className="mb-3 list-disc pl-5 text-sm leading-relaxed last:mb-0">{children}</ul>
  ),
  ol: ({ children }) => (
    <ol className="mb-3 list-decimal pl-5 text-sm leading-relaxed last:mb-0">{children}</ol>
  ),
  li: ({ children }) => <li className="mb-0.5">{children}</li>,
  blockquote: ({ children }) => (
    <blockquote className="mb-3 border-l-2 border-border/60 pl-4 italic text-muted-foreground/80 last:mb-0">
      {children}
    </blockquote>
  ),
  pre: ({ children }) => (
    <pre className="mb-3 overflow-x-auto rounded bg-muted/50 px-3 py-2 last:mb-0">
      {children}
    </pre>
  ),
  code: ({ children, className }) => {
    const isBlock = !!className;
    return isBlock ? (
      <code className="font-mono text-xs leading-relaxed">{children}</code>
    ) : (
      <code className="rounded bg-muted/50 px-1 font-mono text-xs">{children}</code>
    );
  },
  hr: () => <hr className="my-4 border-border/30" />,
  strong: ({ children }) => <strong className="font-semibold">{children}</strong>,
  em: ({ children }) => <em className="italic">{children}</em>,
};

// ─── Component ────────────────────────────────────────────────────────────────

export function BilingualView({ segments, translationSegments, format }: BilingualViewProps) {
  const tsMap = useMemo(() => {
    const m = new Map<string, TranslationSegment>();
    for (const ts of translationSegments) m.set(ts.sourceSegmentId, ts);
    return m;
  }, [translationSegments]);

  const isTxt = format === "txt";

  const sourceContent = useMemo(() => {
    if (isTxt) return segments.map((s) => s.sourceText).join("\n");
    return segmentsToMarkdown(segments, (s) => s.sourceText);
  }, [segments, isTxt]);

  const translationContent = useMemo(() => {
    if (isTxt) {
      return segments.map((s) => tsMap.get(s.id)?.translatedText ?? "").join("\n");
    }
    return segmentsToMarkdown(segments, (s) => tsMap.get(s.id)?.translatedText ?? "");
  }, [segments, tsMap, isTxt]);

  if (segments.length === 0) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        文档为空
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      <div className="grid min-h-full grid-cols-2 divide-x divide-border/20">
        {/* Source */}
        <div className="px-8 py-6">
          {isTxt ? (
            <pre className="whitespace-pre-wrap font-sans text-sm leading-relaxed">
              {sourceContent}
            </pre>
          ) : (
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>
              {sourceContent}
            </ReactMarkdown>
          )}
        </div>

        {/* Translation */}
        <div className="px-8 py-6 text-muted-foreground/90">
          {isTxt ? (
            <pre className="whitespace-pre-wrap font-sans text-sm leading-relaxed">
              {translationContent}
            </pre>
          ) : (
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={mdComponents}>
              {translationContent}
            </ReactMarkdown>
          )}
        </div>
      </div>
    </div>
  );
}
