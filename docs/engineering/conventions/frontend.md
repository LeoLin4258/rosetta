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
- 工作台遵循“原文 in，译文 out”：主窗口左侧列出项目内原文文件，右侧列出当前原文对应的所有译文文件，不在主工作台内直接渲染长文双栏预览。
- App 全局侧边栏只管理项目；项目内源文件、译文文件、多语言状态和批量选择都放在工作台内部。项目内文件管理默认采用“左原文、右译文”的双栏布局。
- 工作台操作分层：右侧译文行内提供打开、翻译和导出；批量操作只从左侧勾选原文文件开始，不在译文列表里再提供批量勾选。不要提供“只创建不翻译”的译文文件按钮，避免工作流分叉。
- 批量创建并翻译必须先创建/复用所有 `源文件 × 目标语言` 的译文文件，并在列表中显示“排队中”，再逐个执行实际翻译，避免用户无法确认所选目标语言是否已进入队列。
- 双击译文文件打开独立预览窗口，预览窗口只负责“原文 + 当前目标语言译文”的左右对照阅读和导出。
- 控件保持克制，优先清晰和可扫描。
- 不使用装饰性渐变、玻璃拟态、大型 hero、泛 AI 产品文案。
- 长文档预览必须使用虚拟滚动。

## Routing

当前使用 hash router，原因是 Tauri 桌面应用不依赖服务端 history fallback。

现有页面：

```txt
/          空白首页
/new       新项目 / 导入
/jobs      任务
/jobs/:jobId/files/:fileId  当前源文件工作台
/preview/:jobId/sources/:sourceFileId  独立原文预览窗口
/preview/:jobId/translations/:translationFileId  独立译文预览窗口
/settings  设置
```

新增页面时，应先确认它是长期导航入口，还是某个 feature 内的局部状态。不要为短期弹窗或 tab 直接增加全局路由。

当前项目和当前源文件以路由为事实来源。侧边栏高亮、任务工作台加载、文件列表选择都应优先读取 `/jobs/:jobId/files/:fileId`，Zustand 中的 `activeJobId` / `activeSourceFileId` 只作为同步后的应用状态和旧路径回退，不应让异步 store 写入覆盖当前路由。当前译文文件在主工作台内是轻量选择状态；原文阅读器使用 `/preview/:jobId/sources/:sourceFileId`，双栏阅读器使用 `/preview/:jobId/translations/:translationFileId`，两者都是独立窗口深链接，不作为主窗口导航入口。

## State

当前使用 Zustand。

规则：

- Store 中保存应用状态和跨页面状态。
- 纯 UI 临时状态优先放组件本地。
- 不把大型文档全文长期塞进 React 组件状态。
- 大文档内容后续应以任务缓存和增量读取为主。
- 应用级设置使用 Zustand persist 持久化，当前包括主题模式和 RWKV 连接配置。
- `setActiveBundle` 只用于用户显式打开、导入或切换到某个 job 的场景。翻译批次保存、导出刷新、重命名刷新等后台结果应使用不抢 active job/file 的 bundle refresh 行为。

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
- 新增 shadcn 组件时使用 `pnpm dlx shadcn@latest add <component>`。
- 主应用侧边栏基于 shadcn sidebar block 和 `src/components/ui/sidebar.tsx`，业务入口在 `src/components/app-sidebar.tsx` 中定制。
- 侧边栏信息架构固定为：顶部 `新项目`，中间项目列表，底部 `设置`。`新项目` 指向 `/new`，应用启动默认进入 `/` 空白首页。不要把导入、任务、设置作为同一层工作台导航混放。
- 桌面侧边栏宽度为 `14.4rem`，即 shadcn 默认 `16rem` 的 90%。不允许通过中间 rail 调整宽度，`SidebarRail` 当前不渲染。
- 侧边栏展开/合并动画使用 CSS width/position transition，当前为 `duration-300 ease-out`，菜单文本使用 opacity transition 辅助隐藏。
- Windows 桌面端使用 Tauri `windowEffects.mica` 和自绘标题栏。主题模式支持 `light`、`dark`、`system`，并同步到 Tauri window theme。注意：`tauri.conf.json` 的默认窗口 theme 使用 `Dark` / `Light`，前端运行时 `setTheme` API 使用 `"dark"` / `"light"` / `null`。外层 app wrapper 和 `body` 必须保持透明，标题栏与侧边栏通过半透明 `--sidebar` token 露出系统材质，主内容区保持 `bg-background` 以保证长文本阅读对比度。
- Mica 的壁纸采样强度由 Windows 控制。Rosetta 通过 `--sidebar`、`--sidebar-primary`、`--sidebar-accent` 的 alpha 值控制前端覆盖层透明度；调低 alpha 会让桌面颜色更明显。
- 窗口标题栏由 `src/components/window-title-bar.tsx` 渲染。不要改回原生 decorations，除非新增 ADR 说明原因。
- `src/components/ui/sidebar.tsx` 的 desktop fixed sidebar 必须从 `--window-titlebar-height` 下方开始，不能延伸到 title bar 后面，否则半透明 `--sidebar` 会在左上角叠加两次并造成色差。
