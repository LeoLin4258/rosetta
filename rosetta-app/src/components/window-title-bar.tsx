import { getCurrentWindow } from "@tauri-apps/api/window";
import { exit } from "@tauri-apps/plugin-process";
import { MinusIcon, SquareIcon, XIcon } from "lucide-react";

import { Button } from "@/components/ui/button";
import { desktopPlatform } from "@/lib/desktopPlatform";
import { cn } from "@/lib/utils";

const appWindow = getCurrentWindow();

export function WindowTitleBar() {
  const isLinux = desktopPlatform === "linux";

  async function startDrag(event: React.MouseEvent<HTMLDivElement>) {
    if (event.button !== 0) {
      return;
    }

    if (event.detail === 2) {
      await appWindow.toggleMaximize();
      return;
    }

    await appWindow.startDragging();
  }

  return (
    <div
      className="flex h-9 shrink-0 select-none items-center bg-sidebar text-sidebar-foreground"
      data-slot="window-titlebar"
    >
      <div
        className="flex h-full flex-1 items-center px-3 text-sm"
        onMouseDown={startDrag}
      >
        {/* <span className="font-medium">Rosetta</span> */}
      </div>

      <div className={cn("flex h-full items-center", isLinux && "gap-1.5 pr-2")}>
        <Button
          aria-label="Minimize window"
          className={cn(
            "h-full rounded-none px-3",
            isLinux && "size-6 rounded-full bg-foreground/7 p-0 hover:bg-foreground/13"
          )}
          onClick={() => void appWindow.minimize()}
          size="icon"
          type="button"
          variant="ghost"
        >
          <MinusIcon className={cn(isLinux && "size-3.5")} />
        </Button>
        <Button
          aria-label="Maximize window"
          className={cn(
            "h-full rounded-none px-3",
            isLinux && "size-6 rounded-full bg-foreground/7 p-0 hover:bg-foreground/13"
          )}
          onClick={() => void appWindow.toggleMaximize()}
          size="icon"
          type="button"
          variant="ghost"
        >
          <SquareIcon className={cn("size-3", isLinux && "size-2.5")} />
        </Button>
        <Button
          aria-label="Close window"
          className={cn(
            "h-full rounded-none px-3 hover:bg-destructive/20! hover:text-destructive",
            isLinux &&
              "size-6 rounded-full bg-foreground/7 p-0 hover:bg-destructive! hover:text-destructive-foreground!"
          )}
          onClick={() => void exit(0)}
          size="icon"
          type="button"
          variant="ghost"
        >
          <XIcon className={cn(isLinux && "size-3.5")} />
        </Button>
      </div>
    </div>
  );
}
