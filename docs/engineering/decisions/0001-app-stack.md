# 0001 App Stack

## Status

Accepted

## Context

Rosetta 是本地优先的长文本翻译桌面应用。它需要访问本地文件、管理本地任务缓存、连接本机 RWKV API，并提供适合长文档预览的桌面端界面。

项目初始状态是通过 `create-tauri-app` 创建的 Tauri v2 + React + TypeScript + Vite 空项目。

## Decision

Rosetta 采用以下基础技术栈：

- Tauri v2 作为桌面壳和本地系统能力边界
- React + TypeScript 作为前端 UI 层
- Vite 作为前端构建工具
- Tailwind CSS 作为基础样式系统
- shadcn/ui 作为 UI 组件源码体系
- shadcn preset `bJMSkhvs`，主题色使用 `stone`
- Zustand 作为轻量状态管理
- React Router 作为页面路由
- `@tanstack/react-virtual` 支持大文档虚拟滚动
- lucide-react 提供常规工具图标
- pnpm/Corepack 作为包管理入口

## Consequences

- 前端功能应优先在 React/TypeScript 层表达，只有涉及本地文件、系统对话框、任务持久化或进程管理时才进入 Tauri/Rust 层。
- 长列表和双语预览必须按虚拟滚动设计，不能直接渲染全部 segment。
- 状态管理先保持轻量，MVP 阶段不引入复杂数据层。
- UI 应保持桌面工具气质，不做聊天式 AI 产品布局。
- 新增通用 UI 控件时应优先通过 shadcn CLI 添加组件源码，再在业务组件中组合使用。
- 前端样式应优先使用 shadcn/Tailwind semantic tokens，例如 `bg-background`、`text-muted-foreground`、`border-border`，避免直接写固定色值。

## Verification

初始基础设施改动后已通过：

```bash
corepack pnpm typecheck
corepack pnpm build
cargo check
```

shadcn 接入后已通过：

```bash
corepack pnpm typecheck
```

`corepack pnpm build` 由用户本地自行执行。
