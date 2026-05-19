# Rosetta PDF v1 — Phase 3 实施计划（切换到 PDFMathTranslate 端到端方案）

> 状态：2026-05-19 与用户对齐确认，approved by user。Phase 2（前端双 PDF 预览 + Docling sidecar）已在 main 上跑通；本 phase 把 Docling 抽取 + pdfium 回填这条路完全替换为 PDFMathTranslate (pdf2zh) 端到端方案。

## Context

**为什么再 pivot 一次**：当前 Docling 抽取 + pdfium-render 回填的方案技术上跑通了，但在真实 PDF 上的格式还原质量只够"基础纯文字"——任何带表格 / 多栏 / 公式 / 复杂排版的 PDF 都会失真。继续在这条路上推进意味着自己手搓 reflow / 表格内换行 / 公式保护 / 字号自适应 / 跨栏一致性——每一项都是独立工程量，违背用户"不要自己手搓，要用成熟方案"的核心诉求（见 memory：[project-pdf-v1-priorities]）。

**切到什么**：[PDFMathTranslate (pdf2zh)](https://github.com/PDFMathTranslate/PDFMathTranslate)，AGPL-3.0 的端到端 PDF 翻译工具（EMNLP 2025 Demo，33k+ star，活跃）。核心引擎是 BabelDOC + DocLayout-YOLO，原生输出"译文 mono.pdf + 双栏 dual.pdf"。pdf2zh.com 在线 demo 验证质量好（学术论文还原度接近原文）。

**调研结论（2026-05-19）**：
- License：AGPL-3.0 — 用户已确认 Rosetta 接受任何 OSS 协议，包括 AGPL（撤销旧"禁用 mupdf/oxidize-pdf"判断）
- 体积：估 **600–900 MB**（vs 当前 Docling 2.1 GB），裁掉 gradio/flask/celery，well under 1.5 GB 上限
- RWKV 接入：pdf2zh 原生支持 `-s openai --openai-base-url`，但 rwkv-mobile 在 mac arm64 上跑 WebRWKV 后端只暴露 `/v1/batch/chat`（非 OpenAI 格式），需要一个 Rust 内嵌 HTTP shim 做格式转译
- Sidecar 契约：CLI `pdf2zh input.pdf -li en -lo zh -s openai -o outdir` 输出 `<name>-mono.pdf`（纯译文）+ `<name>-dual.pdf`（双栏）—— 直接喂给现有 PdfPane 渲染管线，零修改

**用户已对齐的关键 UX 决策**：
- **PDF 在 v1 完全"黑盒"**：去掉 "block 列表视图 + 逐段编辑译文" 功能，PDF 翻译变成"导入 → 翻译 → 双 PDF 预览"三步。译文不满意 → "重新生成"按钮重跑（带 `--ignore-cache`）
- 非 PDF 格式（markdown / txt）的 block 列表 + 编辑路径**完全不动**

**这是第三次方案变更**，用户明确"不想再做半截转向"。本次切换必须一次到位，前两次的代码（Docling Python sidecar 框架、pdfium 回填生成）作废 ~1900 行 Rust + Python 抽取层。

## Goals / Non-Goals

**Goals**
- PDF 端到端走 pdf2zh：用户点翻译 → 后端 invoke pdf2zh sidecar → pdf2zh 通过 OpenAI shim 调本地 RWKV → 输出 mono.pdf → 前端 rasterize 显示
- pdf2zh 作为可下载 sidecar pack（~800 MB），首次 PDF 操作时弹安装引导（复用 managed_rwkv 已有的 install / lifecycle 框架）
- 前端 PdfPane / PdfDocumentPreview / rasterize 后端**完全保留**——它们已经 format-agnostic
- 删干净 Docling 模块 + pdfium 回填生成代码（~1900 行 Rust）

**Non-Goals**
- 逐段编辑译文（v1 黑盒，未来 v1.x 通过 pdf2zh 的 `--prompt` 模板或 TranslationCache 预填重启）
- 实时 segment 级进度（pdf2zh 是页级 tqdm 输出，进度从精细变粗）
- BabelDOC v2 / `--mode precise` 后端（v1 用 pdf2zh 1.x 稳定线，BabelDOC 留作 A/B 实验）
- Windows / Linux pack 分发（v1 mac arm64 only，跟 RWKV 现状一致）
- 自定义 pdf2zh translator 插件（v1 走标准 `-s openai` 路径 + HTTP shim，避免侵入式改动）

## 实施步骤

### 3a. pdf2zh sidecar pack profile + install/lifecycle 复用

复用 [managed_rwkv](../../../rosetta-app/src-tauri/src/managed_rwkv) 框架（install.rs 520 行 / lifecycle.rs 380 行 / layout.rs 160 行 / profile.rs 220 行 / status.rs 260 行）几乎全部是 generic 的，把 `RuntimeProfile` 抽象稍微泛化即可复用。

**新建模块** `rosetta-app/src-tauri/src/managed_pdf2zh/` — 镜像 managed_rwkv 结构：

```
managed_pdf2zh/
  mod.rs       — re-exports + state registration
  profile.rs   — Pdf2zhProfile { pack_url, pack_sha256, pack_size, bin_relative_path }
  layout.rs    — <app-data>/pdf2zh-sidecar/{pack/, runtime-state/, logs/}
  install.rs   — 下载 / SHA256 校验 / 解压 .tar.zst pack（复用 .part / .part.broken resume 逻辑）
  lifecycle.rs — CLI per-job spawn（pdf2zh 是 CLI 不是 daemon，区别于 RWKV）
  status.rs    — Pdf2zhInstalled / Pdf2zhRunning state enum
```

**重要权衡**：pdf2zh 是 **CLI 工具**，每次翻译 spawn 一个进程；不像 docling-serve 那样常驻 HTTP server。所以 `lifecycle.rs` 不需要 health-wait / port-pick / persistent registry —— 只需要 `spawn_pdf2zh_job(input, output, src_lang, tgt_lang) -> Result<PathBuf>`，async 等待退出。比 managed_rwkv 简单一半。

**Pack 结构**（解压后）：
```
<app-data>/pdf2zh-sidecar/pack/
  python/              — python-build-standalone 3.12 arm64 (~50 MB)
  python/lib/python3.12/site-packages/
                        pdf2zh/, babeldoc/, pymupdf/, onnxruntime/, ...
  bin/pdf2zh           — 入口脚本，通过相对路径调 python -m pdf2zh
  models/
    DocLayout-YOLO.onnx — 预打包的布局模型（~12 MB），避免运行时 HF 下载
```

**Pack 制作**（不在本 phase，单独的离线脚本）：
- 用 `uv pip install --python-platform=macosx_11_0_arm64 pdf2zh` 到一个干净 venv
- 删 gradio / gradio_pdf / flask / celery / redis / argostranslate / mcp
- 删 onnxruntime CUDA / DirectML providers
- 预下载 DocLayout-YOLO ONNX 到 `models/`
- tar + zstd 压缩

**下载地址**：GitHub Release（profile.pack_url），HF mirror 留作国内 fallback——这部分逻辑 `install.rs` 已有（见 [managed_rwkv/install.rs](../../../rosetta-app/src-tauri/src/managed_rwkv/install.rs)）

**前端触发**：跟 RWKV 一样，懒加载——用户首次点"翻译"按钮且 format=pdf 时检查 `get_pdf2zh_status`，未安装则弹安装引导 UI（复用 RWKV onboarding 组件结构）

### 3b. OpenAI-compat HTTP shim（RWKV ↔ pdf2zh 翻译桥）

**问题**：pdf2zh `-s openai` 期待标准 `/v1/chat/completions`；rwkv-mobile WebRWKV 后端只有 `/v1/batch/chat`（带 `\nChinese: ` 前缀的特殊格式，见 memory：[rwkv-mobile 翻译响应格式]）。

**方案**：Rosetta backend 起一个 axum HTTP server 监听 ephemeral port，作为 pdf2zh 的 OpenAI 端点。每次 pdf2zh job 启动前拉起 shim，job 结束后停掉。

**新建** `rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs`（预计 ~150 行）：

```rust
pub struct OpenAiShim {
    port: u16,
    join_handle: tokio::task::JoinHandle<()>,
}

pub async fn spawn_shim(rwkv_base_url: String) -> Result<OpenAiShim, ShimError>;
// 暴露 POST /v1/chat/completions
// 内部转发到 rwkv_providers::mobile_batch_chat::translate_batch
// 单条消息 → 单条 batch（pdf2zh 默认 -t 4，并发 4 个请求；rwkv-mobile clamp 后跑串行 OK）

impl Drop for OpenAiShim { /* abort handle */ }
```

复用 [rwkv_providers/mobile_batch_chat.rs](../../../rosetta-app/src-tauri/src/rwkv_providers/mobile_batch_chat.rs) 的 `translate_batch()`。已经有 `/v1/chat/roles` + `/v1/batch/chat` 编排逻辑，shim 调用它即可。

**Dep 新增**：`axum = "0.7"` 加到 Cargo.toml（如果还没有）。Rosetta 已经在用 `reqwest` 做 HTTP client；axum 做 server 端是同生态。

### 3c. 替换 `parse_pdf()` + `generate_rosetta_translated_pdf()` 为 pdf2zh 调用

**改动文件** `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/`：

**新建** `pdf2zh_invoke.rs`（~200 行）：
```rust
pub(crate) async fn invoke_pdf2zh(
    app: &AppHandle,
    source_path: &Path,
    output_dir: &Path,
    src_lang: &str,
    tgt_lang: &str,
    ignore_cache: bool,
) -> Result<Pdf2zhOutput, PdfError> {
    // 1. ensure pdf2zh pack installed (status check, error if not)
    // 2. spawn openai_shim::spawn_shim(rwkv_url) → port
    // 3. spawn pdf2zh subprocess:
    //    env: OPENAI_BASE_URL=http://127.0.0.1:{shim_port}/v1
    //         OPENAI_API_KEY=dummy
    //         OPENAI_MODEL=rwkv
    //    args: pdf2zh <source> -li <src> -lo <tgt> -s openai -o <outdir> -t 1
    //          [--ignore-cache]
    // 4. read stderr line-by-line, parse tqdm progress, emit Tauri event
    //    'rosetta-pdf2zh-progress' { jobId, phase: 'parse'|'translate'|'render', percent }
    // 5. wait for exit; assert returncode 0
    // 6. shim drop on scope exit
    // 7. return Pdf2zhOutput { mono_pdf: PathBuf, dual_pdf: PathBuf }
}

pub(crate) struct Pdf2zhOutput {
    pub mono_pdf: PathBuf,   // <outdir>/<name>-mono.pdf
    pub dual_pdf: PathBuf,   // <outdir>/<name>-dual.pdf
}
```

**改写** `extract.rs::parse_pdf()`：
- 旧：调 Docling 或 pdfium-chars fallback，输出 blocks + segments
- 新：**直接调用 pdf2zh** 同时完成抽取 + 翻译，输出 mono.pdf；blocks/segments **空数组**（PDF 黑盒，无 block）
- 实际上：把 `parse_pdf()` 改为只做 `pre_flight()` 检查 + 返回 empty blocks，把 pdf2zh 调用挪到翻译触发时

但更干净的做法：**拆分 import 和 translation**。
- `parse_pdf()` → 改名 `pre_flight_pdf()`，只检查文件 + 拷源 PDF 到 job 目录，返回 0 blocks
- 翻译命令 `translate_rosetta_job` 在 format=pdf 分支调 `pdf2zh_invoke::invoke_pdf2zh`，结果写到 `<job_dir>/translated.pdf`
- `generate_rosetta_translated_pdf` 命令（"重新生成"按钮）→ 重新调 pdf2zh + `--ignore-cache`

**改写** `rosetta_jobs/mod.rs::generate_rosetta_translated_pdf()`：
- 旧：调 `formats::pdf::render_translated_pdf()` 用 pdfium 自己拼
- 新：调 `pdf2zh_invoke::invoke_pdf2zh()` 重新跑 pdf2zh，覆盖 `<job_dir>/translated.pdf`

**改写** translation 触发路径（job 完成时 / 用户点翻译时）：
- 当前 PDF 翻译是 segment-by-segment 跑 RWKV → 最后 `render_translated_pdf` 合成
- 新：**整段跳过 segment translation**，直接 `invoke_pdf2zh` 一次完成
- 这意味着 `rosetta_jobs/` 里 PDF 的"翻译"命令路径完全 fork：format=pdf 走 pdf2zh，其他格式走旧的 segment-by-segment RWKV

### 3d. 前端 UX 调整

**PdfDocumentPreview.tsx 改动很小**：
- "重新生成" 按钮逻辑不变（调 `generateRosettaTranslatedPdf`，后端语义变成"重跑 pdf2zh"）
- 进度显示：监听新事件 `rosetta-pdf2zh-progress { phase, percent }`，三段进度条
  - phase=parse："正在分析版面..."
  - phase=translate："正在翻译... X%（pdf2zh 内部页级进度）"
  - phase=render："正在生成 PDF..."
- 翻译按钮 + segment 进度面板对 PDF 隐藏

**DocumentPreview.tsx 顶部分发**改动：
```tsx
if (document.format === "pdf") {
  return <PdfDocumentPreview .../>;  // 已存在，无变化
}
// 其他格式走 block 列表 + 编辑路径
```

**Block 列表 UI 对 PDF 隐藏**：
- `WorkspacePage.tsx` 的 segment-by-segment 进度面板对 format=pdf 不渲染
- 任何 block 编辑 / segment retry 按钮对 format=pdf 不渲染
- 简单做法：在 segment 列表组件入口判断 `if (format === 'pdf') return null`

**安装引导 UI**：复用 RWKV onboarding 组件结构 `rosetta-app/src/features/onboarding/`，新增 `Pdf2zhOnboarding` step（用户首次导入 PDF + 点翻译时触发）。

### 3e. Cleanup（删除 ~1900 行 Rust）

**删除整个文件**：
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/docling/` — 整个目录（mod.rs + sidecar.rs + extract.rs，共 964 行）
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/extract.rs` (465 行) — pdfium-chars 抽取，被 `pre_flight_pdf` + pdf2zh 替代
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/generate.rs` (462 行) — pdfium 回填，被"copy pdf2zh 输出"替代

**保留**：
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/rasterize.rs` — 预览 PNG 渲染，pdfium-render 还在用
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/runtime.rs` — pdfium 绑定 + CJK 字体定位
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/errors.rs` — 加 `Pdf2zhFailed(String)` 变体

**Cargo.toml**：
- 保留 `pdfium-render`（rasterize 仍用）
- 保留 `image = "0.25"`（rasterize PNG 编码）
- 新增 `axum = "0.7"`（shim server）
- 删除任何 docling-only dep（grep 一下）

**资源**：
- 保留 `resources/pdf-sidecar/pdfium/` (libpdfium.dylib + 字体)
- `resources/pdf-sidecar/fonts/SourceHanSansCN-Regular.otf` — 旧 pdfium 回填用，**删除**（pdf2zh 自己处理字体）
- 整体 `resources/pdf-sidecar/` 改名 `resources/pdfium/`（更准确）

**State 管理**：
- `lib.rs` 删 `DoclingSidecarRegistry::default()` `.manage()` 调用
- 删除 Docling 相关 Tauri 命令注册
- 注册新命令：`get_pdf2zh_status` / `install_pdf2zh_pack` / `cancel_pdf2zh_install` / `get_pdf2zh_install_progress`（基本镜像 RWKV 命令名）

### 3f. 字段清理

`rosetta-app/src-tauri/src/rosetta_jobs/model.rs`:
- `RosettaBlock.style.pdf.{page, bbox, baselineY, fontSize, layoutConfidence}` 这些字段**只在 PDF 走 Docling 时**填值。新流程下 PDF 没有 block，所以这些字段对 PDF 不再被生成
- **保留字段定义**（schema 向后兼容旧 job 的 document.json），但 importer 不再写

`RosettaDocument.extraction_status` Phase 2 加的字段：
- pdf2zh 是同步翻译（用户点翻译才跑），不再有"导入后台抽取"阶段
- **可保留**字段，但 PDF 导入时直接置 `"done"`，因为抽取被并入翻译阶段

## 新建 / 修改 / 删除文件清单

### 新建

**Rust 后端**：
- `rosetta-app/src-tauri/src/managed_pdf2zh/mod.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/profile.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/layout.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/install.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/lifecycle.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/status.rs`
- `rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs`
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs`

**前端**：
- `rosetta-app/src/features/onboarding/Pdf2zhOnboarding.tsx`
- `rosetta-app/src/lib/pdf2zhRuntime.ts` — 镜像 `rwkvRuntime.ts` 的 status / install / progress 客户端封装

**打包脚本**（不入 git 主线，单独 worktree 或 ops repo）：
- `tools/build-pdf2zh-pack.sh` — uv pip install + slim + 下 ONNX + tar zst
- `tools/upload-pdf2zh-pack.sh` — 上传 GH Release + HF mirror

### 修改

- `rosetta-app/src-tauri/src/lib.rs` — 删 docling state，注册 pdf2zh 命令
- `rosetta-app/src-tauri/Cargo.toml` — 加 `axum`，删 docling-only deps
- `rosetta-app/src-tauri/src/rosetta_jobs/mod.rs`:
  - `generate_rosetta_translated_pdf` → call `pdf2zh_invoke::invoke_pdf2zh` instead of `render_translated_pdf`
  - PDF 翻译命令路径分支：format=pdf → invoke_pdf2zh，其他 → 现有 segment 路径
- `rosetta-app/src-tauri/src/rosetta_jobs/import.rs`:
  - `import_pdf_skeleton` 不再启 Docling 后台任务；导入仅缓存源 PDF
  - 删除 `spawn_pdf_extraction` / `run_pdf_extraction` 函数（功能并入 invoke_pdf2zh）
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/mod.rs`:
  - 删 `mod docling; mod extract; mod generate;`
  - 加 `mod pdf2zh_invoke;`
- `rosetta-app/src/features/workspace/WorkspacePage.tsx` — format=pdf 不渲染 segment 进度面板 + 不渲染 block list
- `rosetta-app/src/features/preview/PdfDocumentPreview.tsx` — 监听 `rosetta-pdf2zh-progress` 事件，三段进度
- `rosetta-app/src/app/AppShell.tsx` — 订阅 pdf2zh 安装 + 翻译进度事件

### 删除

- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/docling/` — 整个目录（964 行）
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/extract.rs` — 465 行
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/generate.rs` — 462 行
- `resources/pdf-sidecar/fonts/SourceHanSansCN-Regular.otf` — pdf2zh 自带字体

**总删除**：~1900 行 Rust + 1 个字体（~9 MB）

## 验证

```bash
cd rosetta-app

# Rust
cd src-tauri
cargo check
cargo test --lib formats::pdf::rasterize  # 保留的部分
cargo test --lib managed_pdf2zh           # 新建
cargo test --lib openai_shim              # 新建（mock RWKV 端）

# TS
cd ..
pnpm typecheck

# 集成 dev run（pack 用本地 venv 临时替代）
ROSETTA_PDF2ZH_BIN=/path/to/local/.venv/bin/pdf2zh pnpm tauri dev
```

**人工验证清单**：
1. 选 PDF → 侧边栏 < 1s 出现 job entry，左侧立即渲染源 PDF（沿用 Phase 2 即时反馈，pdf2zh 在翻译时才介入）
2. 点"翻译"：
   - 若 pdf2zh 未安装 → 弹安装引导，下载 ~800 MB pack
   - 已安装 → 状态变 "正在分析版面..." → "翻译中 X%" → "生成 PDF..."
3. 完成后右侧渲染 mono.pdf，格式还原（学术论文为重点验证：选 1 篇 arxiv paper）
4. 左侧选中文字 → 复制 → 是源文 ✓
5. 右侧选中文字 → 复制 → 是译文 ✓（pdf2zh 输出保留文本层）
6. 点"重新生成"→ 右侧重新跑（带 --ignore-cache，覆盖缓存）
7. 翻译过程中关闭 app → 重新打开 → job 状态恢复 / pdf2zh 进程已被收割
8. 删除 job + 重新导入 → 流程正常
9. 验证 block 列表 UI 对 PDF 已隐藏；对 markdown 仍正常
10. 在断网情况下：源 PDF 可预览；翻译报错（pdf2zh 自身有重试，超时后报"翻译失败"，用户可重试）

**质量基线 PDF**（dogfood 集）：
- 1 篇 arxiv CS paper（双栏 + 表格 + 公式 + 引用）
- 1 份产品 PDF（单栏 + 图片混排）
- 1 份扫描件（pdf2zh 没 OCR，应失败 / 输出空，正确报错）

## 风险与开放问题

1. **AGPL 传染**：切完后 Rosetta 整体变 AGPL。如果未来想做闭源 Pro 版需要拆 pdf2zh 出来跑 sidecar 进程隔离（AGPL §13 网络通信豁免）或买 Artifex PyMuPDF 商业 license。**现在切=不可逆**。用户已确认接受。
2. **学术论文之外的还原质量**：pdf2zh 训练目标是 LaTeX/学术论文。一般 PDF（杂志、商业报告）质量未知，UI 需提示"对学术论文优化，其他类型可能效果有限"。
3. **Pack 制作工程量**：~800 MB pack 制作脚本（slim + ONNX 预下 + tar zst）需要单独 1–2 天，本计划未细化。如果用户优先级是先验证端到端跑通，可以先用本地 venv（`ROSETTA_PDF2ZH_BIN` env var）跑通整链路，pack 制作放 Phase 3b。
4. **pdf2zh 失败重试 / 超时**：长 PDF（>50 页）+ 本地 RWKV 串行可能 30–60 分钟。需要：
   - 后端不设硬超时（让 pdf2zh 自己用 tenacity 重试单段）
   - 前端显示估算时间 + 提示"长文档需要耐心等待"
   - 用户主动 cancel 路径（kill pdf2zh 进程，shim drop）
5. **OpenAI shim 并发**：pdf2zh `-t 4` 默认 4 线程并发翻译；shim 转发到 mobile_batch_chat 时如果 RWKV batch size = 1（mac arm64 默认），4 并发会被串行化。设 `-t 1` 关并发更稳，但翻译时间 ×4。需要在 invoke 命令里固定 `-t 1`，未来 RWKV 支持大 batch 时再放开。
6. **pdf2zh 版本锁定**：`pdf2zh==1.9.11`（pyproject 当前版本）。新版本可能 CLI flag 变更；pack 制作时锁版本号，升级走单独 phase。
7. **数据兼容性**：旧 job（Phase 2 时 Docling 抽取过的）`document.json` 有 blocks 数据；新代码读老 job 时这些 blocks 应该被忽略而不是渲染——加个版本判断（`schemaVersion` 或检查 `extraction_status` 标记）。
8. **rwkv-mobile 模型适配**：pdf2zh 的默认 prompt 是"translate to {lang_out}"；rwkv-mobile WebRWKV 模型对短输入仍可能出词典条目（memory：[rwkv-translate 短输入续写]）。可能需要自定义 `--prompt prompt.txt` 模板加上明确指令。验证窗口：3c 调通后即知。

## 实施顺序（建议 2–3 周）

### Week 1: 端到端跑通（用 local venv，跳过 pack）

1. **3a-lite**: managed_pdf2zh 模块骨架（profile + layout + status），但 install 暂时硬编码读 `ROSETTA_PDF2ZH_BIN` env var；lifecycle 只做 spawn CLI
2. **3b**: OpenAI shim（axum + reqwest），独立 cargo test 模拟 RWKV 端
3. **3c**: pdf2zh_invoke + 替换 parse_pdf / generate；本地 arxiv paper dogfood 跑通
4. **3d**: 前端 UX（移除 block list + 三段进度）

✅ Week 1 退出条件：本地 venv 模式下，arxiv 论文 import → translate → 双 PDF 预览 → export 全链路 dogfood 跑通。

### Week 2: Pack 制作 + 真实安装路径

5. 写 `tools/build-pdf2zh-pack.sh`；产出 ~800 MB pack
6. **3a-full**: install.rs（下载 + 校验 + 解压），复用 managed_rwkv install 模式
7. **3a-onboarding**: Pdf2zhOnboarding 前端组件
8. 第一次 GitHub Release（mac arm64 v0.1.0-pdf2zh-pack）

✅ Week 2 退出条件：用户在干净 mac 上首次导入 PDF → 走完安装引导 → 完成翻译。

### Week 3: 清理 + 风险打磨

9. **3e**: 删除 docling + extract.rs + generate.rs；运行 cargo check / cargo test 确认无回归
10. **3f**: 字段清理 + 旧 job 兼容性测试
11. 长文档（50+ 页 arxiv）性能 + cancel 路径测试
12. 风险 4 / 5 / 8 实测

✅ Week 3 退出条件：删干净，没有 dead code；长文档 + cancel 路径 OK；commit + ship。

**每个 step 完成跑 `cargo check` + `pnpm typecheck` + 简短手测**，不积累问题。
