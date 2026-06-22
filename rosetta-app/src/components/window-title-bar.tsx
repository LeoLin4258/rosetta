import { getCurrentWindow } from "@tauri-apps/api/window";
import { exit } from "@tauri-apps/plugin-process";
import { MinusIcon, SquareIcon, XIcon } from "lucide-react";

import { Button } from "@/components/ui/button";

const appWindow = getCurrentWindow();

export function WindowTitleBar() {
  async function startDrag(event: React.MouseEvent<HTMLDivElement>) {
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

      <div className="flex h-full items-center">
        <Button
          aria-label="Minimize window"
          className="h-full rounded-none px-3"
          onClick={() => void appWindow.minimize()}
          size="icon"
          type="button"
          variant="ghost"
        >
          <MinusIcon />
        </Button>
        <Button
          aria-label="Maximize window"
          className="h-full rounded-none px-3"
          onClick={() => void appWindow.toggleMaximize()}
          size="icon"
          type="button"
          variant="ghost"
        >
          <SquareIcon className="size-3"/>
        </Button>
        <Button
          aria-label="Close window"
          className="h-full rounded-none px-3 hover:bg-destructive/20! hover:text-destructive"
          onClick={() => void exit(0)}
          size="icon"
          type="button"
          variant="ghost"
        >
          <XIcon />
        </Button>
      </div>
    </div>
  );
}
