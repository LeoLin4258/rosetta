import { NavLink, Outlet, useLocation } from "react-router-dom";
import { ShieldCheck } from "lucide-react";
import { navigationItems } from "./navigation";

const pageTitles: Record<string, string> = {
  "/": "导入",
  "/jobs": "任务",
  "/settings": "设置",
};

export function AppShell() {
  const location = useLocation();
  const title = pageTitles[location.pathname] ?? "Rosetta";

  return (
    <div className="flex min-h-screen bg-zinc-950 text-zinc-100">
      <aside className="flex w-60 flex-col border-r border-zinc-800 bg-zinc-950">
        <div className="border-b border-zinc-800 px-5 py-5">
          <div className="text-lg font-semibold text-zinc-50">Rosetta</div>
          <div className="mt-1 text-sm text-zinc-500">本地长文本翻译</div>
        </div>

        <nav className="flex-1 space-y-1 px-3 py-4">
          {navigationItems.map((item) => {
            const Icon = item.icon;

            return (
              <NavLink
                className={({ isActive }) =>
                  [
                    "flex h-10 items-center gap-3 rounded-md px-3 text-sm transition-colors",
                    isActive
                      ? "bg-zinc-800 text-zinc-50"
                      : "text-zinc-400 hover:bg-zinc-900 hover:text-zinc-100",
                  ].join(" ")
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

        <div className="border-t border-zinc-800 px-5 py-4 text-sm text-zinc-500">
          <div className="flex items-center gap-2 text-zinc-300">
            <ShieldCheck className="h-4 w-4 text-emerald-400" />
            本地优先
          </div>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-16 items-center justify-between border-b border-zinc-800 bg-zinc-950 px-6">
          <h1 className="text-lg font-semibold text-zinc-50">{title}</h1>
          <div className="text-sm text-zinc-500">Stage 0 / 1</div>
        </header>

        <main className="min-h-0 flex-1 overflow-auto">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
