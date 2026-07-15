import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";

import { desktopPlatform } from "@/lib/desktopPlatform";
import { cn } from "@/lib/utils";

type ResizeDirection =
  | "East"
  | "North"
  | "NorthEast"
  | "NorthWest"
  | "South"
  | "SouthEast"
  | "SouthWest"
  | "West";

const appWindow = getCurrentWindow();

const resizeHandles: Array<{
  className: string;
  direction: ResizeDirection;
}> = [
  { direction: "North", className: "top-0 right-4 left-4 h-2 cursor-n-resize" },
  { direction: "South", className: "right-4 bottom-0 left-4 h-2 cursor-s-resize" },
  { direction: "West", className: "top-4 bottom-4 left-0 w-2 cursor-w-resize" },
  { direction: "East", className: "top-4 right-0 bottom-4 w-2 cursor-e-resize" },
  { direction: "NorthWest", className: "top-0 left-0 size-4 cursor-nw-resize" },
  { direction: "NorthEast", className: "top-0 right-0 size-4 cursor-ne-resize" },
  { direction: "SouthWest", className: "bottom-0 left-0 size-4 cursor-sw-resize" },
  { direction: "SouthEast", className: "right-0 bottom-0 size-4 cursor-se-resize" },
];

export function WindowFrame({
  children,
  className,
  resizable = true,
}: React.PropsWithChildren<{
  className?: string;
  resizable?: boolean;
}>) {
  const isLinux = desktopPlatform === "linux";
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    if (!isLinux) {
      return;
    }

    document.body.classList.add("rosetta-window-framed");
    document.body.classList.toggle("rosetta-window-maximized", isMaximized);

    return () => {
      document.body.classList.remove("rosetta-window-framed", "rosetta-window-maximized");
    };
  }, [isLinux, isMaximized]);

  useEffect(() => {
    if (!isLinux || !resizable) {
      return;
    }

    let disposed = false;
    let unlisten: (() => void) | undefined;

    async function syncMaximized() {
      const maximized = await appWindow.isMaximized();
      if (!disposed) {
        setIsMaximized(maximized);
      }
    }

    void syncMaximized().catch(() => {});
    void appWindow.onResized(() => {
      void syncMaximized().catch(() => {});
    }).then((stopListening) => {
      if (disposed) {
        stopListening();
      } else {
        unlisten = stopListening;
      }
    }).catch(() => {});

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isLinux, resizable]);

  return (
    <div
      className={cn(
        className,
        isLinux && !isMaximized && "overflow-hidden rounded-[12px]",
        isLinux && isMaximized && "rounded-none"
      )}
    >
      {children}
      {isLinux && resizable && !isMaximized
        ? resizeHandles.map(({ className: handleClassName, direction }) => (
            <div
              aria-hidden="true"
              className={cn("fixed z-50", handleClassName)}
              data-window-resize-handle={direction}
              key={direction}
              onMouseDown={(event) => {
                if (event.button !== 0) {
                  return;
                }
                event.preventDefault();
                event.stopPropagation();
                void appWindow.startResizeDragging(direction);
              }}
            />
          ))
        : null}
    </div>
  );
}
