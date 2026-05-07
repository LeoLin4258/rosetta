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

## Impact

后续功能开发应基于当前骨架继续扩展，不再回到模板式页面结构。

后续通用 UI 控件应优先使用 shadcn CLI 添加的源码组件，业务组件使用 semantic tokens 适配 stone 主题。

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
