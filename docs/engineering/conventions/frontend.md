# Frontend Conventions

## Scope

本文档记录 Rosetta 前端开发约定。当前约定适用于 `rosetta-app/src`。

## Directory Structure

```txt
src/
  app/        应用壳、路由、导航等跨功能结构
  components/ shadcn/ui 组件源码和共享组件
  features/   按业务功能组织的页面和组件
  lib/        通用工具，例如 cn()
  store/      Zustand store
  styles/     全局样式入口
  types/      跨模块共享的领域类型
```

规则：

- `app/` 只放应用级结构，不放具体翻译业务逻辑。
- `features/` 以业务功能命名，例如 `import`、`jobs`、`preview`、`settings`。
- `components/ui/` 由 shadcn CLI 管理，不手写替代组件来绕过 shadcn。
- `lib/utils.ts` 提供 `cn()`，条件 className 使用 `cn()`。
- 跨多个 feature 使用的领域类型放入 `types/`。
- 全局状态先集中在 `store/`，后续复杂后再按功能拆分。

## UI Direction

Rosetta 是本地长文本翻译工作台，不是聊天产品或营销页。

界面原则：

- 第一屏应是可操作的工作台，不做 landing page。
- 使用稳定的侧边导航、顶部标题和内容区结构。
- 控件保持克制，优先清晰和可扫描。
- 不使用装饰性渐变、玻璃拟态、大型 hero、泛 AI 产品文案。
- 长文档预览必须使用虚拟滚动。

## Routing

当前使用 hash router，原因是 Tauri 桌面应用不依赖服务端 history fallback。

现有页面：

```txt
/          导入
/jobs      任务
/settings  设置
```

新增页面时，应先确认它是长期导航入口，还是某个 feature 内的局部状态。不要为短期弹窗或 tab 直接增加全局路由。

## State

当前使用 Zustand。

规则：

- Store 中保存应用状态和跨页面状态。
- 纯 UI 临时状态优先放组件本地。
- 不把大型文档全文长期塞进 React 组件状态。
- 大文档内容后续应以任务缓存和增量读取为主。
- 应用级设置使用 Zustand persist 持久化，当前包括主题模式和 RWKV 连接配置。

## Icons

图标使用 lucide-react。按钮和导航优先使用已有图标，不手写 SVG。

## Styling

Tailwind CSS 和 shadcn/ui 是默认样式方式。全局样式只放入 `src/styles/index.css`，避免分散的全局 CSS。

主题约定：

- shadcn preset 使用 `bJMSkhvs`。
- 主题色使用 `stone`。
- 业务组件优先使用 semantic tokens：`bg-background`、`bg-card`、`text-foreground`、`text-muted-foreground`、`border-border`。
- 不在业务组件里直接使用 `zinc-*`、`emerald-*` 等固定色值作为主要视觉体系。
- 通用按钮、卡片、表格、输入框、选择器、徽标等优先使用 `src/components/ui/` 中的 shadcn 组件。
- 新增 shadcn 组件时使用 `corepack pnpm dlx shadcn@latest add <component>`。
- 主应用侧边栏基于 shadcn sidebar block 和 `src/components/ui/sidebar.tsx`，业务入口在 `src/components/app-sidebar.tsx` 中定制。
- 侧边栏信息架构固定为：顶部 `新项目`，中间项目列表，底部 `设置`。不要把导入、任务、设置作为同一层工作台导航混放。
- 桌面侧边栏宽度为 `14.4rem`，即 shadcn 默认 `16rem` 的 90%。不允许通过中间 rail 调整宽度，`SidebarRail` 当前不渲染。
- 侧边栏展开/合并动画使用 CSS width/position transition，当前为 `duration-300 ease-out`，菜单文本使用 opacity transition 辅助隐藏。
- Windows 桌面端使用 Tauri `windowEffects.mica` 和自绘标题栏。主题模式支持 `light`、`dark`、`system`，并同步到 Tauri window theme。注意：`tauri.conf.json` 的默认窗口 theme 使用 `Dark` / `Light`，前端运行时 `setTheme` API 使用 `"dark"` / `"light"` / `null`。外层 app wrapper 和 `body` 必须保持透明，标题栏与侧边栏通过半透明 `--sidebar` token 露出系统材质，主内容区保持 `bg-background` 以保证长文本阅读对比度。
- Mica 的壁纸采样强度由 Windows 控制。Rosetta 通过 `--sidebar`、`--sidebar-primary`、`--sidebar-accent` 的 alpha 值控制前端覆盖层透明度；调低 alpha 会让桌面颜色更明显。
- 窗口标题栏由 `src/components/window-title-bar.tsx` 渲染。不要改回原生 decorations，除非新增 ADR 说明原因。
- `src/components/ui/sidebar.tsx` 的 desktop fixed sidebar 必须从 `--window-titlebar-height` 下方开始，不能延伸到 title bar 后面，否则半透明 `--sidebar` 会在左上角叠加两次并造成色差。
