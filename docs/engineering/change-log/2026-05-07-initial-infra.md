# 2026-05-07 Initial Infrastructure

## Summary

完成 Rosetta 项目开发前的第一轮基础设施搭建：仓库基础文件、Tauri 模板清理、前端依赖和应用骨架。

## Changes

- 新增根目录 `README.md` 和 `.gitignore`
- 清理 create-tauri-app 默认示例：
  - 删除 Vite/Tauri/React logo
  - 删除 `greet` Tauri command
  - 删除默认 welcome 页面和 `App.css`
- 更新 Tauri 应用元信息：
  - `productName` 改为 `Rosetta`
  - `identifier` 改为 `com.rosetta.desktop`
  - 窗口尺寸改为更适合桌面工作台的默认值
- 新增前端基础依赖：
  - Tailwind CSS
  - shadcn/ui
  - Zustand
  - React Router
  - `@tanstack/react-virtual`
  - lucide-react
- 初始化 shadcn：
  - 使用 preset `bJMSkhvs`
  - 主题色为 `stone`
  - 新增 `components.json`
  - 新增 `@/*` import alias
  - 新增 `src/components/ui/`
  - 新增 `src/lib/utils.ts`
- 新增前端目录骨架：
  - `src/app`
  - `src/components`
  - `src/features`
  - `src/lib`
  - `src/store`
  - `src/styles`
  - `src/types`
- 新增 Rosetta 核心类型草案：
  - `RosettaDocument`
  - `RosettaBlock`
  - `Segment`
  - `RosettaJob`
  - `RwkvConnectionConfig`
- 新增应用壳和初始页面：
  - 导入页
  - 任务页
  - 设置页
  - segment 虚拟滚动预览组件
- 将初始页面迁移为 shadcn 组件组合：
  - Button
  - Card
  - Table
  - Input
  - Select
  - Toggle Group
  - Separator
  - Badge
- 接入 shadcn `sidebar-10` block：
  - 保留 `ui/sidebar` primitives
  - 使用 `SidebarProvider`、`SidebarInset`、`SidebarTrigger`
  - 将示例侧边栏替换为 Rosetta 专用导航
  - 删除 block 自带的无关示例业务组件
- 启用 Windows Mica 系统材质：
  - Tauri 主窗口设置 `transparent: true`
  - Tauri 主窗口设置 `decorations: false`
  - Tauri 主窗口设置 `shadow: true`
  - Tauri 主窗口默认设置 `theme: "dark"`
  - Tauri 主窗口设置 `windowEffects.effects: ["mica"]`
  - App 外层和 `body` 保持透明
  - 标题栏和侧边栏使用半透明 sidebar token
  - 主内容区保持不透明背景
- 新增自绘窗口标题栏：
  - `src/components/window-title-bar.tsx`
  - 支持拖动窗口
  - 支持双击最大化
  - 支持最小化、最大化、关闭
  - 在 capabilities 中添加必要的 `core:window:*` 权限
- 修正 shadcn sidebar 与自绘标题栏的透明背景叠加问题：
  - `SidebarProvider` 设置 `--window-titlebar-height`
  - desktop sidebar fixed container 从 title bar 下方开始
  - sidebar gap 高度扣除 title bar 高度
- 优化侧边栏交互：
  - 桌面宽度从 `16rem` 缩小到 `14.4rem`
  - `SidebarRail` 不再渲染，避免出现中间调整/切换区域
  - 展开/合并动画改为 `duration-300 ease-out`
  - 菜单文本在 icon 折叠状态下用 opacity 过渡隐藏
- 整理侧边栏信息架构：
  - 顶部只保留 `新项目`
  - 中间显示项目列表
  - 底部显示 `设置`
  - 移除底部 `本地优先`
- 新增应用主题设置：
  - 支持浅色、深色、跟随系统
  - 主题设置使用 Zustand persist 持久化
  - AppShell 根据主题模式切换 `.dark`
  - Tauri window theme 同步为 `light`、`dark` 或 `null`

## Impact

后续功能开发应基于当前骨架继续扩展，不再回到模板式页面结构。

后续通用 UI 控件应优先使用 shadcn CLI 添加的源码组件，业务组件使用 semantic tokens 适配 stone 主题。

主导航应继续在 `src/app/navigation.ts` 中维护，并由 `src/components/app-sidebar.tsx` 渲染。

不要把根容器或 `body` 改回不透明背景，否则 Windows Mica 材质会被遮住。不要恢复原生 title bar，否则标题栏和侧边栏的材质颜色会重新出现落差。

当前应用还没有真实文件导入、RWKV 连接、任务持久化或文档解析能力。现有页面是为了固定产品结构和开发边界。

## Verification

已验证：

```bash
corepack pnpm typecheck
cargo check
```

未验证：

- shadcn 接入后未由 Codex 运行 `corepack pnpm build`，由用户本地自行执行
- 未启动 `pnpm dev`
- 未启动 `tauri dev`
- 未做浏览器或桌面端视觉检查

## Notes

在受限 shell 中启动 Vite dev server 时，esbuild 子进程启动被 `EPERM` 拦截。开发服务器由用户本地自行启动。
