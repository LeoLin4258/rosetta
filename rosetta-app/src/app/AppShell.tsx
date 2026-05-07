import { NavLink, Outlet, useLocation } from "react-router-dom";
import { ShieldCheck } from "lucide-react";
import { navigationItems } from "./navigation";
import { Separator } from "@/components/ui/separator";
import { cn } from "@/lib/utils";

const pageTitles: Record<string, string> = {
  "/": "导入",
  "/jobs": "任务",
  "/settings": "设置",
};

export function AppShell() {
  const location = useLocation();
  const title = pageTitles[location.pathname] ?? "Rosetta";

  return (
    <div className="dark flex min-h-screen bg-background text-foreground">
      <aside className="flex w-60 flex-col border-r bg-sidebar text-sidebar-foreground">
        <div className="px-5 py-5">
          <div className="text-lg font-semibold">Rosetta</div>
          <div className="mt-1 text-sm text-muted-foreground">本地长文本翻译</div>
        </div>
        <Separator />

        <nav className="flex-1 space-y-1 px-3 py-4">
          {navigationItems.map((item) => {
            const Icon = item.icon;

            return (
              <NavLink
                className={({ isActive }) =>
                  cn(
                    "flex h-10 items-center gap-3 rounded-md px-3 text-sm transition-colors",
                    isActive
                      ? "bg-sidebar-accent text-sidebar-accent-foreground"
                      : "text-muted-foreground hover:bg-sidebar-accent/60 hover:text-sidebar-accent-foreground"
                  )
                }
                end={item.path === "/"}
                key={item.path}
                to={item.path}
              >
                <Icon className="h-4 w-4" />
                {item.label}
              </NavLink>
            );
          })}
        </nav>

        <Separator />
        <div className="px-5 py-4 text-sm text-muted-foreground">
          <div className="flex items-center gap-2 text-foreground">
            <ShieldCheck className="size-4" />
            本地优先
          </div>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-16 items-center justify-between border-b bg-background px-6">
          <h1 className="text-lg font-semibold">{title}</h1>
          <div className="text-sm text-muted-foreground">Stage 0 / 1</div>
        </header>

        <main className="min-h-0 flex-1 overflow-auto">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
