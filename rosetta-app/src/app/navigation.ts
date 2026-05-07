import { Files, ListChecks, Settings } from "lucide-react";
import type { LucideIcon } from "lucide-react";

export type NavigationItem = {
  label: string;
  path: string;
  icon: LucideIcon;
};

export const navigationItems: NavigationItem[] = [
  { label: "导入", path: "/", icon: Files },
  { label: "任务", path: "/jobs", icon: ListChecks },
  { label: "设置", path: "/settings", icon: Settings },
];
