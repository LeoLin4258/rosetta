import { Link, useLocation } from "react-router-dom";
import { Cpu, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useRosettaStore } from "@/store/useRosettaStore";

/**
 * Subtle one-line banner shown at the top of `<Outlet />` when:
 *   - the user is on Apple Silicon (status NOT `unsupported`),
 *   - the managed RWKV runtime is `not-installed`,
 *   - the user hasn't dismissed it this session,
 *   - and we're not already on `/settings` (no point nudging there).
 *
 * Deliberately not a popup / toast. The main workbench stays focused on
 * documents; this banner is one click of context and one click to dismiss.
 */
export function ManagedRwkvOnboardingBanner() {
  const status = useRosettaStore((s) => s.managedRuntime.status);
  const dismissed = useRosettaStore((s) => s.managedRuntime.bannerDismissed);
  const dismiss = useRosettaStore((s) => s.dismissManagedRuntimeBanner);
  const location = useLocation();

  if (dismissed) return null;
  if (!status) return null;
  if (status.state !== "not-installed") return null;
  if (location.pathname.startsWith("/settings")) return null;

  return (
    <div className="flex items-center justify-between gap-3 border-b bg-muted/40 px-5 py-2 text-xs text-muted-foreground">
      <div className="flex min-w-0 items-center gap-2">
        <Cpu className="size-3.5 shrink-0" />
        <span className="truncate">
          本地 RWKV 翻译尚未安装。一次下载（约 1.3 GB）后即可离线翻译，文档不离开本机。
        </span>
      </div>
      <div className="flex shrink-0 items-center gap-1">
        <Button asChild variant="ghost" size="sm" className="h-7 text-xs">
          <Link to="/settings#local-rwkv">去设置安装</Link>
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="size-7"
          aria-label="收起本地 RWKV 提示"
          onClick={() => dismiss()}
        >
          <X className="size-3.5" />
        </Button>
      </div>
    </div>
  );
}
