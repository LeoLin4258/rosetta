import {
  useCallback,
  useLayoutEffect,
  useState,
  type CSSProperties,
  type RefCallback,
  type ReactNode,
} from "react";

import { cn } from "@/lib/utils";

export function useMeasuredContentWidth<T extends HTMLElement>() {
  const [contentNode, setContentNode] = useState<T | null>(null);
  const [contentWidth, setContentWidth] = useState<number | null>(null);
  const contentRef: RefCallback<T> = useCallback((node) => {
    setContentNode(node);
  }, []);

  useLayoutEffect(() => {
    if (!contentNode) return;

    const updateWidth = () => {
      const nextWidth = Math.ceil(
        Math.max(contentNode.scrollWidth, contentNode.getBoundingClientRect().width),
      );
      setContentWidth((current) => (current === nextWidth ? current : nextWidth));
    };

    updateWidth();

    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(updateWidth);
    observer.observe(contentNode);
    return () => observer.disconnect();
  }, [contentNode]);

  const widthStyle: CSSProperties | undefined =
    contentWidth == null ? undefined : { width: contentWidth };

  return { contentRef, widthStyle };
}

export function AnimatedWidth({
  children,
  className,
  contentClassName,
}: {
  children: ReactNode;
  className?: string;
  contentClassName?: string;
}) {
  const { contentRef, widthStyle } = useMeasuredContentWidth<HTMLSpanElement>();

  return (
    <span
      className={cn(
        "inline-flex max-w-full overflow-hidden transition-[width] duration-200 ease-[cubic-bezier(0.22,1,0.36,1)] motion-reduce:transition-none",
        className,
      )}
      style={widthStyle}
    >
      <span
        ref={contentRef}
        className={cn("flex w-max max-w-none flex-none", contentClassName)}
      >
        {children}
      </span>
    </span>
  );
}
