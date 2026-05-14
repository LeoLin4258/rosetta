# Rosetta v1 UX Redesign — Single-File Focus + Onboarding Window

## Context

经过 Phase 0–6 + Phase 7.A 完整功能交付 + 多轮 dogfood 后，产品**功能层面**已经
可用：本地 RWKV 模型一键安装、可启动、能翻译、能停、能导出。但**真实用户首次
体验**几乎没法用：

1. 首次打开后用户**完全不知道下一步该干嘛**——banner 引导太轻、UI 信号都
   在 Settings 深处。
2. 当前 Workspace（JobsPage）面向"多文件项目 + 批量翻译"设计，对绝大多数
   "我有一个文档要翻译"的用户来说选项和概念过载。
3. 整体 UI **dev tooling 味道太重**——典型例子是 Settings 页：技术字段
   （后端 / Provider ID / SHA-256 / 模型路径 / 日志路径）堆在表面、配色密集、
   缺产品感。

v1 UX 重设计的核心目标：

- **聚焦单文件**：批量翻译的 UI 完全去掉，相关数据模型保留但 UI 不暴露。
- **首次启动有引导**：独立窗口的 onboarding wizard，安装完才进主界面。
- **产品化视觉**：移除技术黑话、给空间感、状态语言友好化、Settings 简化。
- **段级编辑保留但收敛**：从独立窗口改为主窗口右侧 drawer，段在原位高亮。

ADR / Phase 0–7 的所有技术决策**不动**——本次只动 UI 与用户路径。

---

## 词汇约定

数据层（Rust / TS 类型）保持现状，**仅约束 UI 层文案**：

| 数据层 | UI 用语（中文） |
| --- | --- |
| `RosettaJob` | （不出现） |
| `RosettaSourceFile` | **文档** |
| `RosettaTranslationFile` | **译文** |
| `Segment` | **段落** |
| `SegmentStatus` | **待翻译** / **翻译中** / **已完成** / **已编辑** / **失败** / **已跳过** |
| 动作 | **翻译** / **重新翻译** / **导出译文** / **导出双语** / **取消** |

UI 中**禁止**出现：Job、Provider、Backend、Runtime、Sidecar、SHA-256、prefab、
batch_size、tokenizer 等任何技术内部词汇——这些可以出现在 Settings 的"详细
信息"折叠区或运行时日志里，但绝不在主路径上。

---

## 视觉与产品化原则

> 这是贯穿所有 phase 的指导原则。每个 PR 提交前对照这张表自检。

1. **状态语言友好化**：不要"未安装·未启动"这种工程化表述，改成"本地翻译
   引擎尚未下载"等用户语言。
2. **技术细节默认隐藏**：模型路径、SHA-256、PID、端口号等技术信息进"详细
   信息"折叠区，默认收起。
3. **呼吸感**：当前 panel 信息密度过高，新设计每个区域内部留更大间距
   （建议 16/24px 倍数）。
4. **空状态要诱导操作**：不要灰字描述，给一个明确的 CTA + 视觉引导（图标/插画）。
5. **状态用图标 + 颜色，不用文字罗列**：未下载/下载中/已就绪/出错四种状态
   用色块 + 图标 + 一行短文案表达，而不是当前的"6 行 dl/dt 表格"。
6. **去掉 mono 字体的"代码块"**：URL、路径等不再用 `<code>` 等宽字体（除非
   是用户要复制的字符串）。
7. **按钮的动词单一**：一个状态一个主操作。"修复并重试 + 直接重启"两个并排
   按钮会让用户犹豫——选一个作为主、另一个进 menu。
8. **Onboarding 视觉感**：参考 Linear / Notion / Raycast 那种引导窗——大字、
   留白、关键 CTA 突出、装完有完成动画 / 进度条平滑。

---

## 决策落定（讨论结论）

| 项 | 决策 |
| --- | --- |
| Onboarding 形态 | **独立 Tauri 窗口**，640×480 居中，不可缩放 |
| Onboarding 触发条件 | `onboardingCompleted` 标志（持久化）+ 模型文件 任一不满足都重弹 |
| Onboarding "用外部 API 跳过"选项 | 提供，但 UI 极度弱化（小灰字链接放角落） |
| Onboarding 中途关闭 | 视为暂停；下次启动自动重开，从断点续传 |
| Onboarding 期间主窗口 | **不开**，避免双窗口状态同步 |
| 批量翻译 UI | **完全去除**；Rust 命令保留为内部 API |
| 项目 / 文件夹概念 | **去除**——拖文件夹时 flatten 出所有支持格式当成多个独立文档 |
| 侧栏内容 | 平铺"最近 5"文档，活动文档高亮；无 tabs、无"打开/关闭"状态 |
| 段级编辑 | **主窗口右侧 drawer 滑出**，段在原位高亮，不再用独立 preview window |
| Workspace 空状态 | 拖文件 / 选文件 + 下方"最近 5" |
| 现有 jobs 数据 | onboarding 完成时**清空**（首发还没真用户，没历史包袱） |
| Settings 重构 | LocalRwkvPanel 简化（去掉安装按钮——安装走 onboarding），技术细节折叠 |
| macOS 菜单栏 | **基础做**：File (Open / Open Folder / Close / Quit)、Edit (标准)、View (toggle sidebar)、Window、Help、Cmd+, → Settings |

---

## 架构总览

### 三个 UI surface（按用户进入频率排序）

```
[首次启动]                 [日常 99% 时间]              [极少进入]
┌──────────┐              ┌──────────────┐            ┌──────────┐
│Onboarding│ ───完成───>  │  Workspace   │ ──Cmd+,──> │ Settings │
│  Window  │              │  Main Window │            │   page   │
└──────────┘              └──────────────┘            └──────────┘
                              ▲   │
                              │  Cmd+W
                              └───┘
                          (开关文档不退出 app)
```

### 状态流

```
launch
  │
  ├─ check: onboardingCompleted && model_file.exists()
  │    │
  │    ├─ yes → open Workspace
  │    │
  │    └─ no  → open Onboarding window
  │              │
  │              ├─ user 完成 → mark onboardingCompleted = true
  │              │              → close Onboarding window
  │              │              → open Workspace window
  │              │
  │              ├─ user 选"外部 API" → mark onboardingCompleted = true
  │              │                      → close Onboarding window
  │              │                      → open Workspace window
  │              │
  │              └─ user 关闭窗口 / 退 app → next launch resume
```

---

## 实施分期

按依赖顺序，每个 phase 是独立 PR-able 单位。

### P1 — Onboarding 独立窗口 + 首次启动判定

**目标**：第一次跑 dev / .app 弹出新窗口，走完安装流程后进主窗口。

修改：

- `src-tauri/tauri.conf.json`：声明第二个 `WebviewWindow`（label="onboarding"，
  640×480，居中，不可缩放，无 sidebar / 装饰简化），默认不创建——按需开。
- `src-tauri/src/lib.rs`：app `setup` hook 里读
  `managed_rwkv::get_managed_rwkv_runtime_status` + 检查
  `onboardingCompleted` 标志（存到 `~/Library/Application Support/.../onboarding.json`
  或 zustand persist），决定开哪个窗口。
- 新建 `src-tauri/src/onboarding.rs`：
  - `get_onboarding_status() -> { completed: bool, can_resume_install: bool }`
  - `mark_onboarding_completed()`
  - `mark_onboarding_skipped_remote()`
- 新建前端 `src/features/onboarding/`：
  - `OnboardingApp.tsx`（路由根 `/onboarding`）
  - `WelcomeStep.tsx`：欢迎 + 一句话定位 + "开始" CTA
  - `InstallStep.tsx`：代理预填（自动检测）+ 安装进度 + 完成态
  - `SkipToRemoteLink.tsx`：弱化的"用自己的 API"小灰字链接
  - `OnboardingDone.tsx`：完成动画 + "翻译你的第一个文档"按钮
- 路由：`src/app/AppShell.tsx` 拆成 Onboarding 路由与 Workspace 路由两套独立
  React 入口；通过 URL 区分（窗口创建时传 `--route=/onboarding`）。
- Onboarding 完成 → Rust 命令关掉 onboarding window + 开 main window。
- 删除 `ManagedRwkvOnboardingBanner.tsx`（不再需要 banner 引导）。

退出条件：

- 删模型 + 清 `onboardingCompleted` → 重启 dev → 弹 onboarding 窗、不弹主窗
- 走完安装 → 关 onboarding 窗 / 开主窗 / 重启后直接进主窗
- 安装一半关闭 onboarding 窗 → 下次启动重开 onboarding 窗、从断点续传

---

### P2 — Workspace 新布局（单文件视角）

**目标**：替换当前 JobsPage 的批量 UI 为单文件双栏对照视图。

修改：

- 新建 `src/features/workspace/WorkspacePage.tsx`，路由 `/workspace`（替代当前
  `/`、`/new`、`/jobs`、`/jobs/:id`）。
- 布局：
  - Left sidebar `WorkspaceSidebar.tsx`：上区 = "最近 5"，每条点击设为活动
    文档。下区 = 顶栏齿轮（→ Settings）。
  - Top bar `WorkspaceTopbar.tsx`：当前文档名 + 翻译 / 取消 / 导出 按钮组。
  - Main pane `BilingualView.tsx`：双栏对照，每行一段，段对齐，状态用细
    色块（细线条 + 颜色，不是大徽章）。
  - Empty state `WorkspaceEmpty.tsx`：拖区 + "选择文件" 按钮 + "最近 5" 卡片。
- 文件导入：
  - 拖单文件 → 直接成为新文档（active）
  - 拖多个文件 → 都成为独立文档，最后一个 active
  - 拖文件夹 → 递归 flatten 支持格式（`.txt` / `.md` / 未来 `.pdf`），每个成为
    独立文档
  - 顶栏菜单 File → Open / Open Folder
- 数据层：每个文档创建一个 `RosettaJob`（单文件 job），但 UI 不暴露 job 概念。
- 删除 `src/features/jobs/JobsPage.tsx` 及批量翻译相关组件。

退出条件：

- 拖 1 / 多 / 文件夹三种方式都能正确导入并出现在最近列表
- 单文件翻译流程顺畅（活动文档 → 点翻译 → 进度 → 完成 → 导出）
- 关闭文档或切换活动文档行为符合直觉

---

### P3 — 段级编辑右侧 drawer

**目标**：把现有 preview window 的段级编辑改造成主窗口内右侧滑出 drawer。

修改：

- 新建 `src/features/workspace/SegmentEditorDrawer.tsx`：
  - 触发：点击译文段（或原文段）
  - 高亮触发段：BilingualView 那一行 ring / bg-accent
  - drawer 宽度 ~480px，可拖拽调节
  - 内容：原文 + 当前译文 textarea + 翻译历史折叠 + "重新翻译" / "保存编辑" /
    "撤销编辑" 按钮
  - 关闭 drawer：点 ⨯ / 点其他段 / Esc
- 删除 `src/lib/translationPreviewWindow.ts`（独立窗口逻辑不再需要）。
- 现有 segment translation history / retranslation Rust 命令保留——drawer
  只是新 UI 入口。

退出条件：

- 段级编辑、重译、查看历史所有现有功能在 drawer 里都能用
- drawer 开关动画流畅，段高亮明确
- 多段连续点击切换 drawer 内容（不每次重开）

---

### P4 — Settings 简化与产品化

**目标**：Settings 从"dev tooling 罗列"变成"少量必要选项"。

修改：

- `src/features/settings/LocalRwkvPanel.tsx`：
  - 移除"安装本地翻译模型"按钮（安装走 onboarding）
  - 状态卡片用单一色块 + 图标 + 一行短文案，不再 6 行 dt/dd
  - 模型信息行（filename / SHA / path）放到"详细信息"折叠区，默认收起
  - "重新安装"动作放到详细信息内（避免主路径上的危险操作）
- `src/features/settings/SettingsPage.tsx`：
  - 段落间距加大
  - "外部 API"配置默认折叠（不是主路径）
  - "下载代理"输入框也归到 onboarding 时填的同一个 store——这里只是允许后期
    覆盖
  - 更新检测、主题保持
- 新建 `src/features/settings/SettingsDetailsCollapsible.tsx` 等抽象，统一
  "默认隐藏的技术信息"模式

退出条件：

- Settings 进去**第一眼看到的不超过 3 个核心信息**（本地翻译状态、主题、
  应用更新）
- 所有技术字段（模型路径 / SHA / 日志 / 端口 / PID）都进折叠区
- 文案做一遍"用户化"重写，确保非工程读得懂

---

### P5 — macOS 菜单栏

**目标**：标准 macOS app 该有的菜单，Cmd 快捷键肌肉记忆能用。

修改：

- `src-tauri/src/lib.rs` 用 Tauri 2 Menu API 注册：
  - **Rosetta** 菜单：About / Settings (Cmd+,) / Quit (Cmd+Q)
  - **File**：Open File (Cmd+O) / Open Folder (Cmd+Shift+O) / Close (Cmd+W)
  - **Edit**：Cut / Copy / Paste / Select All（标准）+ Undo Edit (Cmd+Z 在段
    编辑器内)
  - **View**：Toggle Sidebar (Cmd+\\) / Zoom In / Zoom Out
  - **Window**：Minimize / Close / Bring All to Front
  - **Help**：Rosetta Help（暂时空连接 / 跳 GitHub README）
- 菜单事件 emit 到前端，由 `WorkspacePage` 接住路由对应动作。

退出条件：

- Cmd+O 弹文件选择对话框、Cmd+, 进 Settings、Cmd+\\ 切侧栏可见性
- 标准 macOS 行为正确（Cmd+W 关当前文档而非退 app）

---

### P6 — 数据 / 状态清理 + 持久化整理

**目标**：onboarding 完成时清掉历史数据，让首发用户从干净状态开始。

修改：

- `src-tauri/src/onboarding.rs::mark_onboarding_completed()`：清空
  `~/Library/Application Support/com.rosetta.desktop/jobs/` 整个目录。
- zustand `useRosettaStore`：onboarding 完成时 reset 翻译相关 slice（`jobs`、
  `activeJobId`、`activeFileId`、各种 `active*` 字段）；保留 `themeMode`、
  `rwkv`、`downloadProxy`、`managedRuntime.bannerDismissed`。
- 新增 `recentDocuments: Array<{ id, name, openedAt, jobId }>`（cap 5，FIFO 淘汰）。
  侧栏读这个。
- `managedRuntime.bannerDismissed` slice 移除（不再有 banner）。

退出条件：

- 删模型 + 清 onboardingCompleted → 重启 → onboarding → 完成 → 主窗口
  `~/.../jobs/` 为空
- "最近 5" 列表只显示新建的文档，旧 dogfood 数据彻底清掉
- 后续翻译会写入新的 jobs 数据，"最近"列表正确更新

---

## 文件级 footprint 概览

新增：

- `src-tauri/src/onboarding.rs`
- `src/features/onboarding/`（5–6 个文件）
- `src/features/workspace/`（5–6 个文件）
- 1–2 个 menubar 注册代码

删除：

- `src/features/jobs/JobsPage.tsx` 及 JobsPage 相关子组件（约 1000 行）
- `src/features/settings/ManagedRwkvOnboardingBanner.tsx`
- `src/lib/translationPreviewWindow.ts` 与所有调用

修改：

- `src-tauri/src/lib.rs`：route 决策 + 菜单注册
- `src-tauri/tauri.conf.json`：第二个窗口配置
- `src/features/settings/LocalRwkvPanel.tsx`：去安装按钮 + 信息折叠
- `src/features/settings/SettingsPage.tsx`：布局重排 + 弱化外部 API
- `src/store/useRosettaStore.ts`：加 `recentDocuments`、移除 `bannerDismissed`、
  调整 partialize
- `src/types/rosetta.ts`：加 recent document 类型

不动：

- 所有 `rwkv_*` 模块（rwkv_api / rwkv_providers / managed_rwkv）
- 数据模型 model.rs / store.rs
- 翻译运行 runner（translationRunner.ts + start_rwkv_*_run 命令）
- 段级编辑的 Rust 命令（仅前端入口换）

---

## 风险 / 开放问题

1. **Tauri 2 双窗口的状态同步**：onboarding window 写 `onboardingCompleted` 后，
   main window 启动时读到——理论上是顺序的（onboarding 关→main 开），但要确认
   zustand persist 写盘时机不会让 main 窗读到旧值。最坏情况 onboarding 完成
   时 emit Tauri event 通知 main 启动。
2. **macOS 菜单 Tauri 2 API**：Tauri 2 的菜单 API 还在演进，可能要看具体版本
   支持哪些事件桥接。早期遇到限制就降级到"前端 hotkey listener"。
3. **drawer 替代 preview window 的功能损失**：当前 preview window 是真独立
   窗口，可以多开（每段一个）。drawer 单实例，同时只能编辑一段。这是有意的
   产品权衡（聚焦），但要确认你接受。
4. **flatten 文件夹的 UX**：拖一个含 200 个 .md 的文件夹会一次性塞 200 条到
   "最近"——可能要给"导入 N 个文档"的批量提示 / cap。
5. **现有 dogfood 数据清空时机**：onboarding 完成时一次性清，是否提示用户？
   建议**不提示**（v1 首发就当所有人是新用户），但要在 change-log 写清楚。
6. **macOS 菜单"Settings"和窗口内的齿轮入口**：两个入口同时存在还是只留菜单？
   推荐两个都留（齿轮是 UI 路径、Cmd+, 是肌肉记忆路径）。

---

## 验证

每个 P1–P6 PR 提交前：

- `cargo check` + `cargo test --lib` + `cargo clippy`
- `pnpm typecheck` + `pnpm lint`（如配置）
- 实机走一遍当前 phase 的退出条件
- 视觉自检对照"视觉与产品化原则"那 8 条

整体收尾（P6 完）：

- 删模型 + 清 onboardingCompleted → 完整重走 onboarding → 翻译一个 markdown →
  导出 → 确认所有路径无 dev 味
- 在另一台 Mac（A3a workaround 下）再过一遍同流程
- 视觉走查：把所有 UI 截图发给一个产品同事 / 非工程用户，看反馈

---

## 文档跟进

- 本计划完成时写一份"v1 UX 重设计 retrospective" change-log。
- ADR 0003 / Phase 7.A 系列计划保留——本计划在最后加链接补充"为什么这一波
  UI 重设计来得这么晚"。
- 老 plan 与 change-log（特别是 JobsPage 相关的）不删，加 supersession note。

---

## 接下来一步

按 P1 → P2 → P3 → P4 → P5 → P6 顺序开始。每个 phase 独立 PR-able、独立可
dogfood。预计专注工时 5–6 天，跨周可完成。

P1 开始之前需要先在 dev 端：

1. 看一下 Tauri 2 多窗口 / 菜单 API 现状（5–10 分钟资料浏览）
2. 把"现有 dogfood 数据无需保留"这件事再次确认一遍——动手就回不去了
