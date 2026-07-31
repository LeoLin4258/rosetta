# PDF 稳定化、包体与可维护性治理实施计划

## 文档状态

- 状态：Active
- 创建日期：2026-07-27
- 审计窗口：2026-07-17 至当前 `HEAD`；更早代码只在解释该窗口内的设计来源时取证
- 当前阶段：CP11，进行中
- 当前生产 PDF 执行路径：`pdf2zh` prepare / unit collection / page render，Rosetta Rust 负责本地翻译、任务状态、页产物、预览与导出
- 当前验证基线：仓库 `main` 在 `61ff0ab` 的审计快照；后续 agent 必须重新读取当前 `HEAD`，不能把该 commit 当成永久事实
- 本文是 PDF 稳定化和治理工作的唯一活跃 handoff authority

本文取代以下文档作为“下一位 agent 从哪里继续”的入口，但不删除它们的历史证据：

- `docs/engineering/plans/2026-07-21-pdf-production-refactor-closeout.md`
- `docs/engineering/plans/2026-07-20-pdf-v3-ten-page-benchmark-regression-handoff.md`
- `docs/engineering/plans/2026-07-16-pdf-v3-native-rewrite.md`

如果本文、历史文档和当前代码冲突，按下面的事实优先级处理。

## 事实优先级

1. 当前 production call graph、Tauri command wiring、持久化读写和构建脚本。
2. 在目标平台实际执行得到的测试、pack inventory、hash 和 benchmark。
3. 已接受且仍与代码一致的 ADR，当前主要是 ADR 0077。
4. conventions、pipeline、closeout、change-log 和历史计划，仅作为待核对背景。

文档中的“已实现”“当前”“生产”“authority”不能单独作为完成证据。每个 checkpoint 都必须从代码重新确认入口和消费者。

## 背景与已核实基线

用户已经接受当前 PDF 预解析速度和视觉回填质量。本计划不以再次重写 PDF renderer 为目标。

截至 2026-07-27 审计，代码事实是：

- `WorkspacePage.tsx` 使用 `preparseRosettaPdfPages`、`translateRosettaPdfPages`、legacy page preview 和 `exportRosettaTranslatedPdf`。
- `usePdfV3RunControl.ts` 和 `usePdfV3Preview.ts` 没有 production consumer。
- v3 Tauri commands 仍注册在 command surface，约 51,232 行自研 v3 Rust 和 81,989 行 vendored `pdfium-render` source 仍留在主线。
- `legacy_adapter.rs` 的 adapter 函数只有定义和测试调用，没有 production caller。历史 closeout 中“通过 adapter 保留 native scheduler/store 到生产链路”的描述不符合当前代码。
- Linux profile 仍固定到 `pdf-layout-pack-linux-x64-v2026.07.15.1`，压缩包大小为 `510,388,352` bytes。
- 尚未发布的新 Linux pack 被报告约 1.8 GB，但仓库中没有该 archive、unpacked inventory 或对应 freeze，不能仅凭数字判断增长来源。
- Linux requirements 只锁定直接依赖；传递依赖没有 hash lock。`pip freeze` 是构建输出，不是下一次构建输入。
- Linux builder 没有 Windows builder 已有的完整 runtime/test/header pruning，也没有压缩、解压体积和文件数预算。
- pack 内包含多个 Rosetta 产品并不使用的 cloud/provider SDK；不能在确认 import graph 前直接删除，但必须治理。
- `patch-pdf2zh-color-preservation.py` 通过构建时字符串替换改变安装后的第三方代码。最终行为不只由 PDFMathTranslate commit 决定。
- production translation/render handoff 使用 `mpsc::unbounded_channel<PdfUnitTranslation>`；当前 pending metrics 不包含仍留在 channel 中的译文。
- installer 在下载完成后才核对 archive size；Linux tar 解压没有过程 byte/file quota，解压期间不能响应取消。
- Linux tests 会编译普通 `#[cfg(test)]` 中的 `windows_sys` import，但该 crate 只在 Windows target dependency 下声明。
- 仓库没有主应用的持续集成 workflow；现有 workflow 只覆盖 macOS RWKV sidecar。
- `cargo clippy --all-targets -- -D warnings` 当前失败，PDF v3 占较多诊断；不能把 clippy 宣称为已建立的门禁。
- `data-models.md`、旧 PDF v1 addendum 和部分 v3 文档仍把 v3 描述成生产路径，与代码及 ADR 0077 冲突。

## 治理目标

### 产品目标

- 保持当前已接受的预解析速度、翻译请求计划和视觉回填质量。
- 不引入新的 PDF 用户流程，不恢复 native v3 renderer，不改变本地优先产品边界。
- Linux 发布具备可解释、可复现、可回滚的组件产物。

### 包体目标

- 能精确解释 archive 和 unpacked 体积由哪些文件、wheel、模型和资产组成。
- 同一锁定输入在相同 builder image 中重复构建时产生相同 dependency resolution 和 inventory。
- pack 增长有相对预算、绝对预算和显式例外流程。
- 删除 Rosetta 不需要的 provider 和开发内容，但不以破坏当前 PDF smoke/视觉基线换取体积。

### 可维护性目标

- production PDF 只有一条明确执行路径。
- 构建产物身份覆盖 fork commit、Rosetta patch/build recipe、dependency lock、Python runtime、模型和字体。
- 长文档队列、安装器下载和解压都有明确上限。
- Linux 真实参与编译和测试。
- 未使用 v3 不再通过默认 production IPC 和无条件 dead-code suppression 假装是活跃架构。
- 后续 agent 只需读本文、当前代码和指定 checkpoint，即可安全继续。

## 非目标与冻结边界

除非用户另行批准新的 PDF 架构项目，本计划期间禁止：

- 重写或重新启用 native v3 renderer。
- 改变当前 pdf2zh translation unit collection、页面回填语义或 RWKV batch/chunk policy。
- 为缩包替换 DocLayout 模型、降低模型精度或删除字体覆盖。
- 把 PDF 组件改成云端服务、按需上传文档或远程处理。
- 同一 checkpoint 同时修改 pack dependency、renderer 行为、持久化 schema 和 workbench UI。
- 为每个 checkpoint 新建 ADR、change-log 或新的 handoff 文档。

如果某项治理必须触碰上述边界，当前 checkpoint 必须停止并请求用户确认，不能自行扩大范围。

## 目标生产架构

治理期间认可的 production ownership 是：

```text
React workbench
  -> Rosetta PDF Tauri commands
    -> managed pdf2zh worker
      -> prepareRun / collectUnits / renderPages
    -> Rosetta local RWKV provider
    -> durable pdf_pages.<targetLang>.json and page artifacts
    -> preview / export

PDF v3 native pipeline
  -> historical or experimental only
  -> not a production fallback
  -> not reachable from default workbench controls
```

生产链中的 source of truth、恢复和删除规则仍以代码中的 legacy PDF page/run state 为准。更新 `data-models.md` 和 `pdf-pipeline.md` 是后续独立 checkpoint，不在实施前假设文档已经准确。

## 工作流与交接规则

### Checkpoint 状态

每个 checkpoint 只能使用以下状态：

- `not-started`
- `in-progress`
- `blocked`
- `completed`

同一时间只能有一个 checkpoint 为 `in-progress`。开始新 checkpoint 前，依赖项必须全部 `completed`，或本文明确允许并行。

### 每位 agent 开始时必须做

1. 阅读仓库根 `AGENTS.md`、本文和当前 checkpoint。
2. 运行 `git status --short`，保留并避开用户已有修改。
3. 记录 `git rev-parse --short HEAD`。
4. 重新通过 `rg` 确认计划中提到的入口、消费者和脚本仍存在。
5. 将本 checkpoint 状态改为 `in-progress`，在本文末尾 execution ledger 写开始记录。
6. 只实施本 checkpoint 的 scope。

### 每位 agent 结束时必须做

1. 运行 checkpoint 指定的验证。
2. 运行 `git diff --check`。
3. 更新 checkpoint 状态和 acceptance checklist。
4. 在 execution ledger 记录：HEAD、修改文件、命令结果、产物路径/hash、未解决问题和下一步唯一动作。
5. 如果 blocked，记录已经尝试的检查和解除阻塞所需的明确外部输入。
6. 不创建新的 handoff 文档；交接信息留在本文。

### 禁止的交接方式

- 只写“基本完成”“应该可以”或“测试通过”，不记录命令与结果。
- 把本机 mutable installed pack 当成 release artifact。
- 用历史 benchmark 数字代替本 checkpoint 的新结果。
- 没有保存 freeze、inventory、hash 就声称 1.8 GB 根因已经确认。
- 因为下游 checkpoint 看起来简单而越过未完成依赖。

## 总体验收门

### 质量不回退

Linux release candidate 和任何 pack dependency/patch 变化必须保持：

- 当前接受的十页真实 PDF fixture 的 page count 和 translation-unit authority。
- 当前可接受的视觉回填结果。
- 当前 RWKV request plan；如果 request count 或 source payload 改变，必须先解释并由用户重新接受。
- prepare cache 命中和 page artifact commit 行为。
- 至少一次 fresh install 和一次 upgrade install 的完整 App 验证。

如果无法访问历史十页 fixture，checkpoint 只能完成自动化部分，不得宣称 visual acceptance 完成。

### 暂定包体预算

CP1 完成前使用以下临时门禁：

- Linux archive hard cap：650 MiB。
- 相对增长 warning：相对上一已发布同平台 pack 增长超过 5%。
- 相对增长 hard fail：增长超过 15%，除非本文 execution ledger 记录用户批准的例外。
- 1.8 GB archive 不得通过修改 hardcoded size/hash 直接发布。
- Linux unpacked baseline：1,353,005,365 bytes；超过 1,420,655,634 bytes（+5%）warning，超过 1,555,956,170 bytes（+15%）hard fail。
- Linux regular-file baseline：21,573；超过 22,652（+5%）warning，超过 24,809（+15%）hard fail。
- Linux 最大单文件 baseline：218,461,128 bytes；出现更大单文件时 warning，超过 256 MiB（268,435,456 bytes）hard fail，除非本文 execution ledger 记录用户批准的例外。
- Linux symlink baseline：1,048；任何增长都 warning，并继续服从 CP5 的 absolute/escaping symlink hard fail。

### 供应链与身份

release pack 必须可以回答：

- 谁构建：builder image/toolchain identity。
- 从什么构建：Rosetta commit、PDFMathTranslate commit、patch/build recipe hash。
- 安装了什么：完整 locked requirements、wheel hashes、SBOM。
- 带了什么资产：Python runtime、model、font 和 license hashes。
- 结果是什么：archive hash、compressed/unpacked bytes、file count 和 inventory hash。

## 实施顺序概览

Linux 发布阻断链：

```text
CP0 -> CP1 -> CP2 -> CP3 -----\
                 \-> CP4 ------+-> CP11
                 \-> CP5 -----/
  \-> CP6 --------------------/
  \-> CP7 -------------------/
```

上图表示依赖关系，不表示多个 agent 同时修改仓库；仍然遵守“同一时间只有一个 `in-progress` checkpoint”。CP4 不依赖 CP3，CP5 不依赖 CP3/CP4，但 CP11 必须等待图中全部门禁完成。

长期可维护性链：

```text
CP0 -> CP6 -> CP8 -> CP9 -> CP10
```

CP8、CP9、CP10 不应与 Linux release candidate 的包内容变化混在同一个 diff 中。执行偏好是先完成 CP11 再开始 CP8；若提前实施 CP8，CP11 必须重新执行所有 pack 与视觉门禁。无论顺序如何，都必须保留明确 owner 和状态。

---

## CP0：冻结当前质量与事实基线

- 状态：`completed`
- 依赖：无
- 建议单次工作量：一个 agent context

### 目标

在任何缩包、依赖或安装器修改前，建立可以检测回归的代码和产品基线。

### Scope

- 只读审计、测试入口整理和必要的轻量测试脚本修正。
- 不修改 renderer、dependency、pack profile 或用户流程。

### 任务

1. 重新确认 production frontend、Tauri commands、Rust orchestration 和 Python worker call graph。
2. 使用 `git log --since=2026-07-17` 和必要的 `git show` 建立本次重构的代码变更清单；历史文档只能帮助定位 commit，不能作为实现证据。
3. 确认 v3 hooks、commands 和 adapter 的实际消费者。
4. 记录当前 Windows/macOS/Linux profile 的 tag、size 和 SHA。
5. 运行现有验证：
   - `pnpm typecheck`
   - `cargo check`
   - `cargo test rosetta_jobs`
   - `python src-tauri/scripts/test-pdf2zh-patches.py -q`
6. 确认真实十页 fixture 是否仍可访问；记录路径只能留在本地 execution ledger，不提交私人文档内容。
7. 记录可复用的质量指标：prepare time、unit count、translatable chars、request count、page artifact bytes、视觉验收状态。

### Acceptance

- [x] production call graph 有代码证据。
- [x] 2026-07-17 起的重构 commit 与实际代码影响已建立映射。
- [x] 验证结果和失败项写入 execution ledger。
- [x] 当前 profile metadata 已记录。
- [x] fixture 可用性和人工验收责任已明确。
- [x] 没有改变 PDF 行为。

### 交付物

- 本文 execution ledger 的 CP0 条目。
- 如必须新增机器可读 baseline，放在现有 benchmark/tooling 目录；不得新增另一份 handoff 文档。

### 停止条件

- 当前 worktree 有与 PDF 重叠且来源不明的用户修改。
- 现有 production call graph 与本文基线显著不同。

---

## CP1：Linux 1.8 GB 产物取证

- 状态：`completed`
- 依赖：CP0
- 建议单次工作量：一个 agent context

### 目标

用 archive 和 inventory 证明体积增长来源，不修改产品代码。

### 必需输入

- 被报告为约 1.8 GB 的 Linux archive，或其构建目录。
- 对应 `requirements.freeze.txt` 和 build log。
- 已发布 2026-07-15 Linux archive，或可下载的 immutable asset。

### 任务

1. 区分 1.8 GB 是 compressed archive、unpacked directory 还是安装后 runtime。
2. 对旧包和新包生成相同格式的 inventory：
   - total bytes、file count、symlink count
   - top-level area bytes
   - top 100 directories/files
   - `site-packages` distribution sizes
   - model/font/runtime sizes
3. diff 两份 freeze，标出新增、升级和传递依赖。
4. 对增长按以下类别归因：Python runtime、wheel、model、font、cache/build residue、tests/headers/static libs、duplicate files。
5. 计算 old/new compressed 与 unpacked ratio。
6. 把最终 unpacked/file-count budget 写回本文“总体验收门”。

### Acceptance

- [x] 1.8 GB 的度量类型已确认。
- [x] 至少 95% 的体积可按目录或 distribution 解释。
- [x] old/new freeze 和 inventory diff 已保存。
- [x] 没有把推测写成根因。
- [x] 最终体积预算已写回本文。

### 停止条件

- 1.8 GB artifact、构建目录和 freeze 都不可获得。此时标记 `blocked`，明确请求所需文件，不允许重建一个不同环境的包后声称复现。

---

## CP2：锁定 Linux pack 输入与构建配方

- 状态：`completed`
- 依赖：CP1
- 建议单次工作量：一个 agent context

### 目标

让 dependency resolution 和所有外部输入可复现，先不做激进缩包。

### Scope

- Linux builder、requirements lock、source/model/font/Python runtime hashes、manifest。
- 不改变翻译和回填行为。

### 任务

1. 生成包含传递依赖和 hashes 的 Linux lock；选择 `uv pip compile` 或 `pip-compile --generate-hashes`，不要同时引入两套工具。
2. 安装时要求 locked hashes，并优先要求 binary wheels。
3. 固定并校验：
   - python-build-standalone archive SHA-256
   - DocLayout model SHA-256
   - font asset SHA-256
   - PDFMathTranslate commit
   - Rosetta patch/build scripts hash
4. 增加 `build_recipe_id`，至少绑定 Rosetta commit、builder script hashes 和 dependency lock hash。
5. 让 freeze 继续作为输出证据，但不再作为唯一解析记录。
6. 在同一 clean builder image 中构建两次，比较 freeze 和 inventory；若要比较 archive SHA，还需控制 tar order、mtime、owner/group 和 gzip timestamp。

### Acceptance

- [x] 无未锁定的 Python distribution。
- [x] 所有下载输入在使用前验证 hash。
- [x] 两次构建的 freeze 和 inventory 一致。
- [x] manifest 可以追溯到完整 build recipe。
- [x] pack smoke 和 CP0 自动化质量基线通过。

### 停止条件

- 某个依赖没有适用的 Linux wheel并触发本地源码构建；必须先记录 toolchain 和产物身份，不能静默接受不可复现 build。

---

## CP3：Linux dependency diet 与安全裁剪

- 状态：`completed`
- 依赖：CP2
- 建议单次工作量：一个 agent context；如 provider import 解耦需要修改外部 fork，应拆到 CP8

### 目标

在不改变 PDF 输出行为的前提下，把 Linux pack 降到预算内。

### 任务

1. 使用 import trace、`pipdeptree --reverse` 和 worker smoke 确认 Rosetta 实际需要的模块。
2. 对 Azure、DeepL、OpenAI、Ollama、Tencent、Xinference 等 provider SDK 逐项证明：
   - production worker 是否直接 import
   - 是否只是 pdf2zh optional provider
   - 删除后是否影响 `rosetta_engine`
3. 如果 pdf2zh eager import 阻止删除 provider，停止删除并把解耦工作转到 CP8，不能用 stub package 欺骗 import。
4. 对 Linux Python runtime 执行与 Windows 对齐的保守裁剪：headers、static libs、Tcl、tests、bytecode、debug symbols 和已证明无用内容。
5. 每轮只改变一类内容，生成 size delta 和 smoke 结果。
6. 保留所有必须 license 文件。

### Acceptance

- [x] archive 和 unpacked size 均在预算内。
- [x] 每个删除项有 import/dependency 证据。
- [x] real prepare/collect/render smoke 通过。
- [x] CP0 translation-unit authority 未变化。
- [x] SBOM 和 license inventory 完整。

### 停止条件

- 任何删除改变 unit count、source payload、rendered page result 或视觉结果。
- 需要修改 renderer heuristic 才能继续缩包。

---

## CP4：pack manifest、engine revision 与兼容性能力

- 状态：`completed`
- 依赖：CP2；可与 CP3 分开实施
- 建议单次工作量：一个 agent context

### 目标

避免不同回填行为的 pack 都只报告 `ENGINE_CONTRACT_VERSION == 2`，让 App 可以明确拒绝缺少必要修补的旧组件。

### 任务

1. 设计向后兼容的 `engineRevision` 或 capabilities 列表。
2. capability 必须反映 App 实际依赖的行为，例如 reusable prepared run、durable layout cache、authoritative render slots、partial-page accounting。
3. worker handshake、pack smoke、installed manifest 和 profile readiness 使用同一能力定义。
4. 旧 manifest 缺字段时给出明确升级路径，不能 panic 或误报 ready。
5. 不把 renderer heuristic 细节逐项暴露给前端。

### Acceptance

- [x] 7 月 15 日旧 Linux pack 和新 pack 能被明确区分。
- [x] 不满足最低 capability 的 pack fail closed，并给出可操作安装提示。
- [x] 新 manifest 保持旧安装记录可解析。
- [x] frontend 不成为 component identity authority。

### 停止条件

- 方案要求破坏现有安装目录或强制删除用户已可用 pack，且没有迁移/升级行为。

---

## CP5：下载、解压和磁盘安全边界

- 状态：`completed`
- 依赖：CP1、CP2
- 建议单次工作量：一个 agent context

### 目标

大型或异常 pack 不能无限下载、无限解压、占满磁盘或在取消后继续长时间运行。

### 任务

1. 下载时在写盘前/过程中执行最大 byte 限制；已知 expected size 时不允许超过 expected size 加很小协议容差。
2. manifest 增加可信 `unpackedSizeBytes`、`fileCount`，并保持旧 manifest 兼容。
3. 解压前检查可用磁盘空间，覆盖 archive、staging、新 pack、旧 pack 和安全余量。
4. ZIP/TAR 解压限制：
   - path traversal
   - symlink/hardlink escape
   - unpacked bytes
   - file count
   - single-file bytes
5. Linux tar 解压必须可取消；不能只在启动 `tar` 前检查 cancel flag。
6. 失败或取消时清理 `.part` 和 extraction staging，同时保留当前已工作的旧 pack。
7. 加入异常 archive 测试，不需要运行 production build。

### Acceptance

- [x] oversized download 在超过限制时立即停止并删除 partial。
- [x] oversized/file-count/path-escape archive 被拒绝。
- [x] extraction cancel 有自动化测试。
- [x] 失败不会先删除旧 pack。
- [x] fresh install 和 upgrade install 通过。

### 停止条件

- 继续调用平台 `tar` 无法实现可靠 quota/cancel。此时应先提出窄化 Rust archive reader 方案，不能用轮询 UI 状态伪装取消。

---

## CP6：Linux CI 与跨平台编译门禁

- 状态：`completed`
- 依赖：CP0
- 建议单次工作量：一个 agent context

### 目标

让 Linux 特有编译错误在发布前自动出现，而不是由用户机器发现。

### 任务

1. 修复 `legacy_adapter.rs` Windows memory probe 的 target gating。
2. 新增主应用 CI，至少在 Linux 执行：
   - frontend install + `pnpm typecheck`
   - `cargo check`
   - `cargo test rosetta_jobs`
   - patch suite
3. Windows/macOS 可先保留为单独 job 或手动 release gate，但 workflow 必须清楚标注平台覆盖差异。
4. 为 package builder 建立 `workflow_dispatch` 或 path-triggered job；避免每个普通 UI PR 下载和构建数百 MiB pack。
5. package job 输出 inventory、SBOM、size-gate result，不自动发布 release asset。

### Acceptance

- [x] Linux test target 编译并通过。
- [x] PR 可以看到主应用基础验证结果。
- [x] pack workflow 默认不发布、不改 profile。
- [x] workflow 失败日志不包含文档文本或凭据。

### 停止条件

- CI 需要新增 secret 或发布权限。基础验证可以继续，发布步骤必须暂停并请求用户授权。

---

## CP7：production 译文队列背压与真实内存指标

- 状态：`completed`
- 依赖：CP0
- 建议单次工作量：一个 agent context

### 目标

消除长文档翻译结果的无界 channel，并让指标覆盖 channel、pending map 和当前 render payload。

### 任务

1. 将 `unbounded_channel<PdfUnitTranslation>` 改为容量有依据的 bounded channel。
2. provider callback 必须获得真实背压；不能在 callback 内 spawn 无界 send task。
3. 记录 queue depth/bytes、pending map units/chars 和 peak combined pending state。
4. 取消和 worker failure 必须能解除 producer/consumer 等待。
5. 添加 producer 快于 renderer、renderer failure、cancel 和 receiver drop 测试。
6. 不改变 provider chunking、unit order或页面 ready 判断。

### Acceptance

- [x] 不存在 production `unbounded_channel<PdfUnitTranslation>`。
- [x] slow renderer 测试中 queue 不超过容量。
- [x] cancel/failure 不死锁。
- [x] unit count、render order 和 CP0 benchmark request plan 不变。
- [x] metrics 明确区分或合并 queue 与 map peak。

### 停止条件

- 当前 callback contract 无法异步等待。此时应先做窄接口调整，不能通过更大的无界容器掩盖问题。

---

## CP8：把 Rosetta Python patch 迁入 PDFMathTranslate fork

- 状态：`not-started`
- 依赖：CP0、CP6；建议在第一个稳定 Linux release candidate 后实施
- 建议单次工作量：一个 agent context 只完成一个可审查 patch family，不要求一次迁移全部 3,000 行

### 目标

最终执行代码由一个明确 fork commit 表达，不再依赖大规模构建时字符串替换。

### 约束

- 这是跨仓库工作。开始前必须确认 PDFMathTranslate fork checkout、branch、worktree 和 upstream tests 状态。
- 不允许把 sibling repo 的未提交修改直接打包。
- 每次只迁移一个有独立测试的 patch family，例如 render-slot authority、color/style、diagram filtering 或 alignment。

### 任务

1. 建立 patch family inventory，列出目标文件、marker、现有 Rosetta tests 和行为风险。
2. 在 fork 中以正常 Python 代码和 upstream-style tests 实现一个 family。
3. Rosetta patch script 对已迁移 family 改为验证 capability，而不是再次 source rewrite。
4. 使用 fresh checkout 构建 pack，不能使用 mutable installed pack。
5. 记录 fork commit、Rosetta commit、pack hash 和视觉验收。
6. 重复 checkpoint，直到字符串 patch 只保留必要的短期兼容层；每次重复仍使用 CP8 子记录，不新建计划。

### Acceptance

- [ ] 本轮 patch family 在 fork 中有正常测试。
- [ ] clean fork commit 足以重建行为。
- [ ] Rosetta builder 不再对该 family 字符串替换。
- [ ] pack smoke 和 visual baseline 不回退。
- [ ] execution ledger 记录两个仓库的精确 commit。

### 停止条件

- fork 有来源不明的未提交修改。
- 迁移改变视觉结果、unit authority 或 request plan。
- 无法取得 upstream/fork test 环境。

---

## CP9：隔离未使用 PDF v3 与默认 IPC surface

- 状态：`not-started`
- 依赖：CP0、CP6；不得与 CP8 同时实施
- 建议单次工作量：一个 agent context 先完成隔离，删除留到后续重复 checkpoint

### 目标

让“代码存在”和“生产启用”有明确区别，降低误调用、误维护和文档漂移。

### 第一步：隔离

1. 从 production Tauri handler 移除没有 production consumer 的 v3 commands。
2. 删除或隔离无消费者的 frontend v3 hooks/types/wrappers。
3. 把 v3 module 放到默认关闭的 Cargo feature，或提出更小的可验证隔离方案。
4. CI 如果保留 v3，增加显式 experimental feature check；默认 production check 不应靠全局 `allow(dead_code)`。
5. 保留 production 仍实际使用的 PDFium raster/source identity primitive，不能按目录整删。

### 第二步：删除决策

在一个稳定 release 后重新 `rg` consumers：

- 如果仍无产品计划和消费者，为 v3 建立明确 git tag/branch snapshot 后从 `main` 删除 dead implementation。
- 如果用户决定重启 v3，必须新建独立架构计划和真实文档 benchmark gate，不能在本治理计划中重新接线。

### Acceptance

- [ ] 默认 workbench 无 v3 command path。
- [ ] production binary command surface 不注册未使用 v3 control/export commands。
- [ ] 删除/feature gate 没有影响 source preview、translated-page preview 或 export。
- [ ] dead-code suppression 范围明显缩小并有理由。
- [ ] v3 历史可以通过 git 精确恢复。

### 停止条件

- 发现生产路径实际依赖某个 v3 primitive。先把该 primitive 的职责和消费者记录清楚，再决定提取或保留，不能整模块删除。

---

## CP10：文档事实收敛

- 状态：`not-started`
- 依赖：CP9 的隔离方案确定；可在 CP11 后完成
- 建议单次工作量：一个 agent context

### 目标

把 PDF 文档从逐提交叙事收敛为少量当前事实，历史文档不再误导新 agent。

### 任务

1. 更新 `docs/engineering/pdf-pipeline.md`，只描述当前 production pdf2zh 路径和已验证恢复/导出规则。
2. 修正 `data-models.md` 中把 v3 描述为当前 production authority 的条目；历史 v3 schema 移到明确 archived section 或独立历史索引。
3. 给旧 v3 plans、closeout 和相关 addendum 增加统一 superseded/archived banner，不重写历史结果。
4. 更新 `docs/engineering/README.md` 文档治理规则：
   - 一个领域只能有一个 active handoff
   - checkpoint 结果回写 active plan
   - 一个 release 最多一个 aggregate change-log
   - ADR 只记录长期不可逆选择
5. 建立短小的 archived PDF document index，列出文档状态，不再复制正文。
6. 全仓 `rg` “PDF v3 是当前”“workbench uses v3”等陈述，逐项核对。

### Acceptance

- [ ] 新 agent 从 engineering README 只能得到一个 active PDF plan。
- [ ] current pipeline、data model 和代码不再在 production engine 上冲突。
- [ ] 历史 benchmark/ADR 保留，但有清楚状态。
- [ ] 本 checkpoint 没有再新增逐任务 change-log。

### 停止条件

- 某项文档修正需要先决定持久化迁移或重新启用 v3。此时只标记冲突，不替代码做架构决定。

---

## CP11：Linux release candidate、回滚与发布交接

- 状态：`completed`
- 依赖：CP1、CP2、CP3、CP4、CP5、CP6、CP7
- 建议单次工作量：一个 agent context 生成 RC；人工 visual acceptance 和实际发布可以由用户完成

### 目标

生成、验证并可安全回滚的 Linux PDF component release candidate，之后才更新 app profile。

### 任务

1. 从 clean Linux builder 构建 release candidate。
2. 生成并保存：archive、SHA-256、signed/checksum file、manifest、locked requirements、freeze、SBOM、inventory、licenses、build log。
3. 执行：
   - in-place smoke
   - relocation smoke
   - fresh install
   - upgrade from 2026-07-15 pack
   - cancel/failure cleanup
   - exact fixture prepare/collect/render
   - full-App translation和人工视觉验收
4. 验证 archive/unpacked/file-count budget。
5. 上传到新的 immutable release tag；不得覆盖旧 asset。
6. 上传成功并重新下载校验后，单独更新 `managed_pdf2zh/profile.rs`。
7. 记录回滚步骤：恢复上一 known-good profile metadata 并重新发 app；不能修改已发布 asset 内容。

### Acceptance

- [x] 所有产物身份和 hash 完整。
- [x] size gate 通过。
- [x] Linux CI 通过。
- [x] fresh/upgrade install 通过。
- [x] 用户视觉验收通过。
- [x] profile 只在 immutable asset 验证后更新。
- [x] 回滚步骤经过至少一次 dry run 或静态验证。

### 停止条件

- 任何 smoke、size、inventory、fresh install、upgrade 或视觉验收失败。
- release asset 尚未上传或重新下载 hash 不一致。
- 需要改变 renderer/RWKV policy 才能通过。

## Release 后观察

Linux 发布后至少观察一个版本，再决定 CP9 第二步删除和 CP8 后续 patch family：

- pack 下载/安装失败率只能来自本地用户反馈和隐私安全日志，不新增遥测。
- 记录实际安装磁盘占用、首次 worker 启动、prepare cache、长文档内存和取消行为。
- 至少完成一次 500 页级 soak；不能仅凭队列有界就宣称 1,000 页认证。
- 任何 visual regression 优先回滚 pack/profile，不在用户已发布版本上远程替换 asset。

## Execution Ledger

本节是唯一实施交接记录。每个 agent 在开始和结束时追加一个简短条目；不要删除以前记录。

### 当前状态摘要

- 当前 checkpoint：CP11（`completed`）
- last completed：CP11
- blocked：无
- 已知 CP11 release issue：无；AppImage 页产物压缩、custom RC schema v2 兼容、committed-source Linux CI、immutable upload/redownload 与 profile update 门禁均已关闭
- last verified HEAD：`9ba4fa9`；最终 profile update 已在该提交之上完成验证
- 下一步唯一动作：推进 CP8，按 patch family 将构建时 patch 迁入 PDFMathTranslate fork；每个 family 独立验证，不与新的 release profile 变更混合

#### 2026-07-28 / CP11 / Codex

- 状态：started
- HEAD：`de9de8f`
- worktree baseline：existing CP7 changes: `docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`, `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs`, `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/unit_translation.rs`, `rosetta-app/src-tauri/src/rosetta_jobs/mod.rs`
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - `git status --short` -> pass；确认仅有上一 checkpoint 的四个未提交文件
  - `git rev-parse --short HEAD` -> `de9de8f`
  - `rg` CP11、release profile、pack builder、artifact compression 与 Linux workflow 入口 -> pass；CP11 依赖 CP1–CP7 均已完成，profile 在 immutable asset 验证前保持冻结
- 产物：
  - pending
- 已确认事实：
  - CP11 必须包含 CP7 已接受的 bounded handoff 改动，但真实 release builder 仍须满足 clean-worktree 和可追溯 source identity
  - 既有 AppImage artifact compression 环境污染是本 checkpoint 的 full-App release blocker
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 定位并修复 artifact compression 环境净化，完成本地与 Linux 聚焦验证后生成可追溯 RC

#### 2026-07-28 / CP11 automated gates / Codex

- 状态：started；等待用户 visual acceptance、immutable upload 与 profile update
- HEAD：`de9de8f`；RC pack 绑定该 committed source identity，AppImage 额外包含上一 checkpoint 未提交 CP7 diff 与本 checkpoint 修复
- worktree baseline：existing CP7 changes preserved；新增 CP11 builder、installer release-gate、artifact compression 与治理文档改动尚未提交
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`、`docs/engineering/change-log/2026-07-27-pdf-linux-pack-reproducibility.md`、`rosetta-app/src-tauri/scripts/build-pdf2zh-pack-linux-x64.sh`、`rosetta-app/src-tauri/src/managed_pdf2zh/install.rs`、`rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/page_artifact_compression.rs`
- 执行命令与结果：
  - isolated Linux pack build with pinned local PBS/model/font/PDFMathTranslate inputs -> pass；in-place、relocation、post-prune real PDF smoke 和 28 runtime imports 通过；final release 仍须在本 checkpoint diff 提交后以 clean worktree 重建
  - first complete archive attempt -> stopped after archive because `set -o pipefail` made `sort -nr | head -1` return SIGPIPE before manifest generation；改为完整读取的 `awk` 后以新 recipe identity 重建通过
  - inventory + machine-readable size gate -> pass；0 failures、0 warnings
  - exact `2604.17278v1.pdf` pages 1–10 `prewarm -> prepareRun -> collectUnits -> identity renderPages` -> pass；94 units、41,035 source chars、canonical unit SHA 与 139,293,175 artifact bytes 保持 CP0 基线
  - real old/RC archive installer release gate -> pass；RC fresh activation 和从 2026-07-15 archive upgrade 均通过，提交后无 backup 残留
  - cancellation、failed-upgrade rollback、manifest rollback focused tests -> pass
  - Linux `pnpm typecheck`、`cargo check`、artifact compression tests、`cargo test rosetta_jobs --lib` -> pass；remote rustfmt component 未安装，format gate 由本地执行
  - Linux AppImage build -> pass；真实 AppImage 在隔离 RC pack 上恢复已有十页任务并完成 artifact compression 10/10、0 failed、0 skipped
  - post-compression independent PDF reopen -> pass；10/10 单页 artifact 均保留可提取文本，总 artifact bytes 为 4,474,336
  - local `pnpm typecheck`、`cargo check`、`cargo test rosetta_jobs`、patch suite、`cargo fmt --all -- --check`、Git Bash `bash -n`、`git diff --check` -> pass；141 Rust tests、44 patch tests
- 产物：
  - RC archive：`/home/rwkv/cp11-dist/rosetta-pdf2zh-linux-x64.tar.gz`；475,162,678 bytes；SHA-256 `02a4e65328d39652a94f62e6035067232a4cdf73b773ad1009ca33e6cfa6c22a`
  - build recipe ID：`a35615e186e42d6c1917fe935da79ebfefcd42ec10aeb5354ea0deaf33ca1c35`
  - manifest/inventory/size gate/build log/SBOM/license inventory/lock/freeze/quality evidence：`/home/rwkv/cp11-dist/`
  - CP0 quality SHA-256：`36990286a6e03c7414a156c0067c02391554d54654e3863e0b065adb9329dbaa`
  - AppImage：`/home/rwkv/Applications/Rosetta-0.1.0-beta.23-cp11-rc.AppImage`；93,018,616 bytes；SHA-256 `e17cd9c9c2fa4554ba33a13fe31868e75109f9d588a889c6a8e6664974b43e03`
- 已确认事实：
  - AppImage compression 子进程现在与 persistent worker 一样移除继承的 `PYTHONHOME`、`PYTHONPATH`，Linux 额外移除 `LD_LIBRARY_PATH`；既有 `ModuleNotFoundError: encodings` release blocker 已在真实 AppImage 中复验关闭
  - live PDF pack 与 `managed_pdf2zh/profile.rs` 未修改；验收 App 使用 `/home/rwkv/cp11-app-data` 下的隔离 RC pack，现有 jobs 被显式复用，因此十页任务的 durable page artifacts 已按既有后台压缩语义更新
  - RC pack regular files 11,104、unpacked 1,262,340,737 bytes、symlinks 1,044、max file 218,461,128 bytes；全部在 CP1 budget 内
- 未解决问题或 blocker：
  - 用户尚未对当前 RC AppImage 完成人工视觉验收
  - immutable GitHub asset 尚未上传/重新下载校验，Linux CI 尚未针对包含 CP7/CP11 的 committed source 运行，profile 因此保持冻结
- 下一步唯一动作：
  - 用户在已打开的 `Rosetta-0.1.0-beta.23-cp11-rc.AppImage` 检查十页中文回填与预览；接受后先提交/运行 Linux CI，再上传新 immutable tag、重新下载校验，最后单独更新 profile

#### 2026-07-30 / CP11 RC managed-pack compatibility / Codex

- 状态：completed；RC managed-pack compatibility blocker 已关闭，CP11 仍等待用户 visual acceptance
- HEAD：`de9de8f`；保留既有 CP7/CP11 worktree diff
- 修改文件：`rosetta-app/src-tauri/src/managed_pdf2zh/layout.rs`、`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - 隔离 RC AppImage 启动与工作台检查 -> fail；已安装 schema v2 custom RC pack 被 UI 误报为“PDF 组件需更新”
  - manifest/profile 核对 -> 定位为 `customPack` 虽跳过 archive SHA/size 的旧 profile 等值校验，但 schema v2 unpacked size/file count 仍无条件对比冻结 profile
  - `cargo fmt --all -- --check` -> pass
  - `cargo test managed_pdf2zh::layout --lib` -> pass；5 passed，新增回归同时确认 custom RC 可携带不同统计，official release pack 仍对 profile 统计 fail closed
  - local `pnpm typecheck`、`cargo check`、`cargo test rosetta_jobs` -> pass；141 Rust tests
  - Linux `pnpm typecheck`、`cargo test managed_pdf2zh::layout --lib` -> pass；5 passed
  - Linux `pnpm tauri build --config src-tauri/tauri.linux.conf.json` -> pass；修复后 AppImage 生成
  - 隔离 RC 数据目录重启与 X11 实机窗口截图 -> pass；主窗口标题为 `Rosetta`，右上角显示绿色“PDF 引擎已就绪”
- 产物：
  - 修复后 AppImage：`/home/rwkv/Applications/Rosetta-0.1.0-beta.23-cp11-rc.AppImage`；93,014,520 bytes；SHA-256 `7393f4cfa62b975b078f96295d633558048b4927f09839bd93ba7098c4a384d3`
  - 修复前可回滚 AppImage：`/home/rwkv/Applications/Rosetta-0.1.0-beta.23-cp11-rc-before-custom-pack-fix.AppImage`；SHA-256 `e17cd9c9c2fa4554ba33a13fe31868e75109f9d588a889c6a8e6664974b43e03`
  - PDF ready 截图：`/home/rwkv/cp11-dist/cp11-main-pdf-engine-ready.png`
- 已确认事实：
  - CP11 custom RC 必须在 immutable upload/profile update 前可验证；其 SHA-256、schema v2 统计完整性和 engine capability 合同仍强制校验
  - 修复不改变 official release pack 与 profile SHA、archive size、unpacked size、file count 必须精确匹配的规则
- 未解决问题或 blocker：
  - 用户尚未对已完成的十页中文回填任务进行人工视觉验收
- 下一步唯一动作：
  - 用户在当前 RC 中选择左侧绿色完成状态的 `2604.17278v1.pdf`，检查十页中文回填与预览

#### 2026-07-30 / CP11 visual acceptance / Leo

- 状态：completed
- 验收对象：修复后 `Rosetta-0.1.0-beta.23-cp11-rc.AppImage`，使用隔离 CP11 RC pack 与十页 `2604.17278v1.pdf` 任务
- 结果：用户确认“回填质量没问题”；CP11 人工视觉验收门禁通过
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 提交当前 CP7/CP11 diff 并在该 committed source 上运行 Linux pack CI

#### 2026-07-30 / CP11 committed-source Linux CI / Codex

- 状态：started；第一次 workflow 启动失败，已定位并修复 runner 执行权限
- committed source：`e34689a`（已 push 至 `origin/main`）
- 执行命令与结果：
  - `gh workflow run build-pdf2zh-pack-linux.yml --ref main` -> dispatched
  - GitHub Actions run `30523689042` -> fail before build；`build-pdf2zh-pack-linux-x64.sh: Permission denied`
  - `git ls-files -s` -> Linux builder 误为 `100644`，而同类 release builder 为 `100755`
- 修复：
  - 恢复 `rosetta-app/src-tauri/scripts/build-pdf2zh-pack-linux-x64.sh` executable bit 为 `100755`；不改变 builder 内容或 recipe identity
- 未解决问题或 blocker：
  - 修复提交后需重新触发 committed-source Linux CI
- 下一步唯一动作：
  - 提交/push executable-mode 修复并重跑 Linux pack workflow

#### 2026-07-30 / CP11 committed-source release / Codex

- 状态：completed
- HEAD：`9ba4fa9`；最终 profile update 在该 committed-source baseline 之上单独提交
- worktree baseline：clean；下载 helper 仅为本机临时文件，不进入提交
- 修改文件：`rosetta-app/src-tauri/src/managed_pdf2zh/profile.rs`、`docs/engineering/change-log/2026-07-27-pdf-linux-pack-reproducibility.md`、`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - GitHub Actions run `30523916772` at `9ba4fa9d25fd896e6c33d06bde49605c492c9ef9` -> pass；build、inventory、size gate 与 artifact upload 全部成功
  - Actions artifact `8751949654` download + outer digest verification -> pass；471,548,064 bytes，SHA-256 `28b8038987bbed36b08c719ed1eef73240d0d11089e9d2ffefe1b045884dcd24`
  - checksum、manifest、inventory、build recipe 与 size gate cross-check -> pass；0 failures、0 warnings，source identity 与 committed HEAD 一致
  - immutable draft upload -> pass；archive 与 11 个 audit sidecars 的远端 size/digest 均与本地逐项一致后才发布
  - published release independent redownload -> pass；475,205,783 bytes，重新计算 SHA、发布 checksum sidecar 与 GitHub asset digest 三方均为 `7ee995e376d9451095939799d2fc2f8fd2691b04f8111fa9ea3cbfc55e626977`
  - `pnpm typecheck`、`cargo fmt --all -- --check`、`cargo check` -> pass
  - `cargo test managed_pdf2zh::profile --lib` -> pass；4 passed
  - `cargo test managed_pdf2zh --lib` -> pass；56 passed，1 个需要显式 old/RC archive 环境变量的真实安装门禁按设计 ignored
  - `cargo test rosetta_jobs` -> pass；141 passed
- 产物：
  - immutable release：`https://github.com/LeoLin4258/rosetta-assets/releases/tag/pdf-layout-pack-linux-x64-v2026.07.30.1`
  - archive：475,205,783 bytes；unpacked 1,262,340,737 bytes；11,104 regular files；1,044 symlinks；max file 218,461,128 bytes
  - build recipe ID：`08d30ed2e219874c9c8878f6e97e517c04767b32d0e0bf717e3f6691555fdbb5`
- 已确认事实：
  - Linux profile 只在 published asset 独立回下载验证后更新，并精确固定 tag、archive size、SHA-256、unpacked size 与 file count
  - 回滚保持静态验证过的旧路径：恢复 `pdf-layout-pack-linux-x64-v2026.07.15.1` profile metadata 并发布新 app build；不得覆盖任一已发布 asset
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 执行 CP8，并保持每个 patch family 与 release/profile 变更解耦

### 记录模板

```text
#### YYYY-MM-DD / CPn / agent

- 状态：started | completed | blocked
- HEAD：<short sha>
- worktree baseline：clean | existing changes: <paths>
- 修改文件：<paths or none>
- 执行命令与结果：
  - `<command>` -> pass/fail/not-run
- 产物：
  - `<path>`
  - size/hash/inventory id
- 已确认事实：
  - ...
- 未解决问题或 blocker：
  - ...
- 下一步唯一动作：
  - ...
```

#### 2026-07-28 / CP7 / Codex

- 状态：started
- HEAD：`de9de8f`
- worktree baseline：clean
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - `git status --short` -> pass；开始前 worktree clean
  - `git rev-parse --short HEAD` -> `de9de8f`
  - `rg` production translation handoff / pending metrics / render payload -> pass；生产入口仍是 `pdf2zh_invoke.rs` 的 `mpsc::unbounded_channel<PdfUnitTranslation>`，callback 为同步 `FnMut`，现有 peak 只覆盖 pending map
- 产物：
  - pending
- 已确认事实：
  - CP0 已完成，CP7 依赖满足
  - 当前 provider callback contract 需要窄化为可异步等待，不能通过 callback 内 spawn task 实现背压
  - CP7 不改变 provider chunking、unit order、页面 ready 判断、renderer 行为、持久化 schema 或 workbench UI
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 实现 bounded translation handoff、真实 combined pending metrics 与 producer/renderer/cancel/drop 测试

#### 2026-07-28 / CP7 completion / Codex

- 状态：completed
- HEAD：`de9de8f`
- worktree baseline：clean；本 checkpoint 的 bounded queue、metrics、tests 与治理文档改动尚未提交
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`、`rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs`、`rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/unit_translation.rs`、`rosetta-app/src-tauri/src/rosetta_jobs/mod.rs`
- 执行命令与结果：
  - `pnpm typecheck` -> pass
  - `cargo check` -> pass
  - `cargo test rosetta_jobs::formats::pdf::pdf2zh_invoke::tests --lib` -> pass；10 passed
  - `cargo test rosetta_jobs::formats::pdf::unit_translation --lib` -> pass；24 passed
  - `cargo test callback_backpressure_preserves_provider_request_plan_and_unit_order --lib` -> pass；1 passed
  - `cargo test rosetta_jobs` -> pass；140 passed，0 failed
  - `cargo fmt --all -- --check`、`git diff --check` -> pass
  - `rg` production `unbounded_channel<PdfUnitTranslation>` -> no matches
- 产物：
  - production translation handoff：32-unit bounded Tokio channel；sender 先等待 capacity permit，再记录入队并发送，不在 callback 内 spawn send task
  - timeline metrics：queue capacity/peak units/payload bytes、pending map peak units/chars/payload bytes、render payload peak units/chars/bytes、combined pending peak units/chars/payload bytes
- 已确认事实：
  - slow renderer test 使用容量 2、5 个 producer events，queue peak 精确为 2 且 FIFO render order 保持 `unit-0..unit-4`
  - renderer failure、cancelled blocked producer 与 receiver drop 都在 1 秒测试上限内解除等待；receiver drop 作为 callback error 向 provider task 传播
  - 容量 1 的 callback backpressure 下 scripted provider 仍执行 3 个请求、batch distribution 为 `[1×1, 2×2]`，五个 unit 的 emission/result order 不变；本 checkpoint 未修改 unit collection、provider chunking、页面 ready 判断或 render semantics
  - queue/map/render payload 使用同一共享 metrics authority 更新；combined peak 计算实际同时存活的三个 payload copy，timeline 不包含原文、译文、路径或凭据
  - 未运行真实十页 fixture 或 full-App visual acceptance；CP7 不改变 pack、renderer heuristic、translation source payload 或 request planning，质量回归由不变 call graph、request-plan/FIFO tests 与完整 Rust suite 覆盖
- 未解决问题或 blocker：
  - CP7 无 blocker；Linux AppImage 页产物压缩子进程环境污染仍是 CP11 release gate 前必须解决的既有问题
- 下一步唯一动作：
  - 执行 CP11，生成 Linux release candidate 并完成 fresh/upgrade、full-App 与人工 visual acceptance 门禁；通过前不得修改 release profile 或发布

#### 2026-07-28 / CP6 / Codex

- 状态：started
- HEAD：`dfe6a07`
- worktree baseline：clean
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - `git status --short` -> pass；开始前 worktree clean
  - `git rev-parse --short HEAD` -> `dfe6a07`
  - `rg` workflows / Linux builder / inventory / SBOM / legacy adapter Windows probe -> pass；仓库只有 macOS sidecar workflow，Linux builder 已输出 manifest 与 SBOM，但没有主应用 CI 或独立 size gate
- 产物：
  - pending
- 已确认事实：
  - CP0 已完成，CP6 依赖满足
  - `legacy_adapter.rs` 的 `#[cfg(test)]` 模块在非 Windows test target 仍无条件 import `windows_sys`
  - CP6 只建立编译和构建治理门禁，不运行 production pack build，不发布 release asset，不修改 `managed_pdf2zh/profile.rs`
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 修复 Windows memory probe target gating，新增 Linux 主应用 CI、手动 pack workflow 与可机读 size gate

#### 2026-07-28 / CP6 completion / Codex

- 状态：completed
- HEAD：`dfe6a07`
- worktree baseline：clean；本 checkpoint 的 workflow、size gate、target gating 与治理文档改动尚未提交
- 修改文件：`.github/workflows/main-app-ci.yml`、`.github/workflows/build-pdf2zh-pack-linux.yml`、`rosetta-app/src-tauri/scripts/check-pdf2zh-pack-size.py`、`rosetta-app/src-tauri/scripts/test-pdf2zh-patches.py`、`rosetta-app/src-tauri/src/pdf_v3/legacy_adapter.rs`、`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - local `pnpm typecheck` -> pass
  - local `cargo check` -> pass
  - local `cargo test rosetta_jobs` -> pass；134 passed
  - local `cargo test pdf_v3::legacy_adapter` -> pass；1 passed、1 ignored Windows manual probe
  - local `python src-tauri/scripts/test-pdf2zh-patches.py -q` -> pass；44 passed
  - local `cargo fmt -- --check`、Python `py_compile`、`git diff --check` -> pass
  - checksum-verified `actionlint v1.7.7` on both workflows -> pass
  - isolated Linux x64 host `pnpm install --frozen-lockfile`、`pnpm typecheck`、`cargo check` -> pass
  - isolated Linux x64 host `cargo test rosetta_jobs` -> pass；103 passed
  - isolated Linux x64 host `cargo test pdf_v3::legacy_adapter` -> pass；1 passed，确认非 Windows test target 不再编译 Windows memory probe
  - isolated Linux x64 host patch suite -> pass；44 passed、1 Linux skip
- 产物：
  - required PR workflow：`.github/workflows/main-app-ci.yml`；SHA-256 `9bb78bb516cc4af6789297f35b5515f022e94e0d0700948c4dacf462d3e36fd5`
  - manual pack workflow：`.github/workflows/build-pdf2zh-pack-linux.yml`；SHA-256 `1c18c09e53a50a931b6b7daa400744701db492989a87c2a737b2c58951fe548a`
  - machine-readable size gate：`rosetta-app/src-tauri/scripts/check-pdf2zh-pack-size.py`；SHA-256 `fdf67b3a51adf7deb0c0b50852e85c73d34a7bc26b2c03efac4e42594dd0b550`
- 已确认事实：
  - 主应用 workflow 在相关 PR/main push 上执行 Linux frontend install/typecheck、`cargo check`、`cargo test rosetta_jobs` 与 patch suite；job 名称和 step summary 明确 Windows/macOS 仍为 manual release gates
  - pack workflow 只有 `workflow_dispatch`，权限为 `contents: read`；成功路径上传 archive、inventory、SBOM、license inventory、size-gate、recipe、locks 与 build log 为 14 天 workflow artifact，不调用 GitHub Release、不修改 profile
  - size gate fail closed，并执行 CP1 的 archive/unpacked/file-count/max-file/symlink budget；current CP3 candidate fixture 通过，700 MiB synthetic archive 被拒绝且保留 JSON result
  - 两个 workflow 不读取 secrets、不接受文档输入，禁用 PDF diagnostics；失败日志只覆盖代码测试和 builder 状态，不包含用户文档文本或凭据
  - 本 checkpoint 未运行 production pack build，未发布 asset，未修改 profile、renderer、translation-unit authority、RWKV request plan、持久化 schema 或用户流程
- 未解决问题或 blocker：
  - none；Linux AppImage artifact compression 环境污染仍为既有 CP11 release issue
- 下一步唯一动作：
  - 执行 CP7，为 production PDF 译文队列建立 bounded backpressure 与真实 combined pending metrics；不得改变 provider chunking、unit order 或页面 ready 判断

#### 2026-07-28 / CP4 / Codex

- 状态：started
- HEAD：`6b4ddb5`
- worktree baseline：clean
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - `git status --short` -> pass；开始前 worktree clean
  - `git rev-parse --short HEAD` -> `6b4ddb5`
  - `rg` worker handshake / installed manifest / profile readiness / pack smoke -> pass；入口仍位于 `managed_pdf2zh/worker.rs`、`install.rs`、`profile.rs` 与 pack build scripts
- 产物：
  - pending
- 已确认事实：
  - CP2、CP3 已完成，CP4 依赖满足
  - CP4 不改变 renderer heuristic、translation-unit authority、持久化页状态或 workbench UI
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 定义并接通 Rust authority 的最低能力集合，使旧 pack fail closed 且保留可升级安装记录

#### 2026-07-28 / CP4 / Codex

- 状态：completed
- HEAD：`6b4ddb5`
- worktree baseline：clean；本条记录覆盖当前 CP4 未提交 diff
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`, `rosetta-app/src-tauri/scripts/pdf2zh-engine-capabilities.json`, `patch-pdf2zh-color-preservation.py`, `test-pdf2zh-patches.py`, 四个 pack build/staging scripts，`managed_pdf2zh/capabilities.rs`, `install.rs`, `layout.rs`, `mod.rs`, `status.rs`, `worker.rs`
- 执行命令与结果：
  - `pnpm typecheck` -> pass
  - `cargo check` -> pass
  - `cargo test rosetta_jobs` -> pass；134 passed
  - `cargo test managed_pdf2zh` -> pass；44 passed
  - `python scripts/test-pdf2zh-patches.py -q` -> pass；41 passed
  - `cargo fmt -- --check`、PowerShell parser、Git Bash `bash -n` 三个 shell builder、`git diff --check` -> pass
- 产物：
  - capability authority：`rosetta-app/src-tauri/scripts/pdf2zh-engine-capabilities.json`；schema 1、contract 2、revision 1；SHA-256 `a73f3f2ec19784609e24df39e81316790683273ab028dcc746db6917d4dc2922`
  - required capabilities：`authoritative-render-slots`, `durable-layout-cache`, `partial-page-accounting`, `reusable-prepared-run`
  - 未运行 release pack production build；CP4 通过 builder smoke wiring、patch suite 与 Rust compatibility tests 验证，不产生新 archive
- 已确认事实：
  - worker handshake、pack root capability manifest、installed manifest 与 profile readiness 都由同一 version-controlled JSON 派生；contract 精确匹配、revision 允许向前、capability 使用 required-subset 规则
  - 新 pack 在替换当前 pack 前验证能力清单；缺字段的旧安装记录仍可反序列化，但 readiness fail closed，并提示用户从设置重新安装
  - installed manifest schema 与 engine capability schema 使用独立字段，frontend 只接收 native readiness/status，不拥有 component identity
  - 本 checkpoint 未修改 renderer heuristic、translation-unit authority、RWKV request plan、页状态 schema 或 workbench UI
- 未解决问题或 blocker：
  - none；Linux AppImage artifact compression 环境污染仍为既有 CP11 release issue
- 下一步唯一动作：
  - 执行 CP5，为 download/extraction byte quota、磁盘空间、path/link 安全与 cancellation 建立门禁

#### 2026-07-28 / CP5 / Codex

- 状态：started
- HEAD：`c3fc561`
- worktree baseline：clean
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - `git status --short` -> pass；开始前 worktree clean
  - `git rev-parse --short HEAD` -> `c3fc561`
  - `rg` download / extraction / manifest / profile readiness -> pass；安装入口仍位于 `managed_pdf2zh/install.rs`
- 产物：
  - pending
- 已确认事实：
  - CP1、CP2 已完成，CP5 依赖满足
  - Linux 当前调用平台 `tar`，运行期间不可取消且没有 unpacked/file/single-file quota
  - 当前升级在新 candidate 验证后直接删除旧 pack，替换失败无法保证旧 pack 保持可用
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 实现受限下载、Rust archive reader、磁盘预检和事务式 pack 替换，并加入异常 archive 与取消测试

#### 2026-07-28 / CP5 completion / Codex

- 状态：completed
- HEAD：`c3fc561`
- worktree baseline：clean；本 checkpoint 的实现与治理文档改动尚未提交
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`、`docs/engineering/change-log/2026-07-27-pdf-linux-pack-reproducibility.md`、`rosetta-app/src-tauri/Cargo.toml`、`rosetta-app/src-tauri/Cargo.lock`、`rosetta-app/src-tauri/src/managed_pdf2zh/{install,layout,profile}.rs`、Linux/macOS/Windows pack builders、local staging script
- 执行命令与结果：
  - `cargo test managed_pdf2zh --lib` -> pass；54 passed，覆盖 oversized download/partial cleanup、ZIP traversal 与 quotas、tar.gz 正常解压与 symlink escape、解压中取消、staging cleanup、fresh/upgrade install、pack 与 manifest rollback
  - `pnpm typecheck` -> pass
  - `cargo check` -> pass
  - `cargo test rosetta_jobs` -> pass；134 passed，0 failed
  - `python src-tauri/scripts/test-pdf2zh-patches.py -q` -> pass；41 passed
  - `cargo fmt -- --check`、`git diff --check` -> pass
  - Windows pack builder PowerShell AST parse -> pass
  - `bash -n` -> unavailable；本机 `bash.exe` 解析到 WindowsApps WSL launcher，两次调用均超时，未继续重试
- 产物：
  - 安装器 650 MiB archive 绝对上限、expected size + 64 KiB streaming tolerance、exact final size/SHA-256 门禁
  - archive/unpacked/old-pack/safety-margin 磁盘预检，以及 ZIP/tar.gz path/link/byte/count/single-file 门禁
  - 可取消 Rust tar.gz reader、`.part`/staging RAII cleanup、pack 与 installed manifest 事务式升级/回滚
  - installed manifest schema 2 capacity evidence；schema 1 readiness 保持兼容；future release/local staging manifests 输出同类指标
- 已确认事实：
  - Linux 已知 release baseline 为 1,353,005,365 unpacked bytes、21,573 regular files；hard limits 为 1,555,956,170 bytes、24,809 files、256 MiB single file
  - fresh install 与 upgrade acceptance 由 synthetic automated install transaction tests 覆盖；没有运行 production pack build 或 runtime UI verification
  - renderer、translation-unit authority、RWKV request plan、capability 集合和用户流程均未改变
- 未解决问题或 blocker：
  - CP5 无 blocker；shell builders 已人工审计，但受本机 WSL launcher 限制未取得 `bash -n` 结果
- 下一步唯一动作：
  - 执行 CP6，建立 Linux CI 与跨平台编译门禁；pack workflow 默认不得发布或修改 profile

#### 2026-07-27 / CP0 / Codex

- 状态：started
- HEAD：`61ff0ab`
- worktree baseline：existing changes: `docs/engineering/README.md`, `docs/engineering/plans/2026-07-21-pdf-production-refactor-closeout.md`, `docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - `git status --short` -> pass；只有本治理计划及其文档 authority 切换改动
  - `git rev-parse --short HEAD` -> `61ff0ab`
- 产物：
  - none
- 已确认事实：
  - CP0 已开始；未修改 production PDF 行为、依赖或 pack profile
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 完成 CP0 production call graph、历史 commit、profile、fixture、质量指标和验证基线取证

#### 2026-07-27 / CP0 / Codex

- 状态：completed
- HEAD：`61ff0ab`
- worktree baseline：existing changes: `docs/engineering/README.md`, `docs/engineering/plans/2026-07-21-pdf-production-refactor-closeout.md`, `docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`；前两项为开始 CP0 前已有的 authority 切换改动，本 checkpoint 未覆盖
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - `rg` production consumers/callers -> pass；`WorkspacePage.tsx:310/676/1010` 分别调用 pdf2zh preparse/translate/export，`PdfDocumentPreview.tsx:184` 调用 legacy translated-page PNG；`lib.rs:228/249/259/266` 注册对应 Tauri commands
  - `rg` Rust/worker call graph -> pass；`rosetta_jobs/mod.rs:1998/2052` 进入 `preparse_pdf2zh`，`:1860/2106/2533` 进入 `invoke_pdf2zh`，`pdf2zh_invoke.rs:366/421/712` 依次 prepare、Rosetta local provider translation、page render，Python worker 在 `rosetta_pdf2zh_worker.py:298/311/388` 调用 `prepareRun`/`collectUnits`/`renderPages`
  - `rg` v3 consumers -> pass；`usePdfV3RunControl.ts`、`usePdfV3Preview.ts` 无 production import，v3 frontend wrappers 只被这两个无消费者 hook 使用；v3 commands 仍在 `lib.rs:229/234/239-240/246/248/253/256/260-262` 注册
  - `rg` legacy adapter callers -> pass；`pdf_v3/legacy_adapter.rs` 仅由 `pdf_v3/mod.rs:14` 声明，函数只有同文件测试调用，无 production caller
  - `git log --since=2026-07-17 --reverse` 与关键 `git show --stat` -> pass；`6e9774d..d383e2f` 建立 native v3 extraction/store/scheduler/preview/export，`d045096`/`7c453c9` 曾把 workbench 切到 v3，`eaac597`/`95dc684` 延伸 region rewrite，`85be312` 恢复 pdf2zh production 并留下未接线 adapter，`be51de6` 加入当前 fill-back/page selection，`bb005e8` 加入当前 legacy page-map preview，`6201b07` 加固 authoritative render slots/partial fallback，`eed557b` 只补 local staging manifest，`b0db2d6` 只补 Linux worker env overrides，`61ff0ab` 更新 release/profile metadata；`a91001c` 是上述分支的 merge commit，无独立 production authority 变化
  - `pnpm typecheck` -> pass
  - `cargo check` -> pass
  - `cargo test rosetta_jobs` -> pass；134 passed, 0 failed
  - `python src-tauri/scripts/test-pdf2zh-patches.py -q` -> pass；34 passed
  - isolated installed-engine `prewarm -> prepareRun -> collectUnits -> identity renderPages` probe -> pass；10/10 pages rendered，临时输出已清理
  - `git diff --check` -> pass；tracked changes 无 whitespace error，只有既有 CRLF conversion warning
  - `git diff --no-index --check -- NUL docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md` -> 新文件预期 exit 1，无 whitespace error，只有 CRLF conversion warning
- 产物：
  - fixture：`C:\Users\Leo\Desktop\pdf-set-1\2604.17278v1.pdf`；3,032,412 bytes；SHA-256 `5db8200931a2d4104cf435a70701e80d47849c201000ed86ca645ab25d454da2`；engine 确认 10 pages
  - current installed custom-pack engine baseline：`rosetta-pdf-engine-v2.1` / contract 2；fresh-process prewarm 1,637.2 ms；cache-miss prepare 3,861.7 ms；94 collected/translatable units；41,035 translatable source chars；canonical compact sorted-key UTF-8 unit JSON SHA-256 `81d6185ffc72f263bbc03a6ab1872e4e8615728ad47ecd359b1b2b1d2f3cecb5`
  - identity render baseline：1,130.1 ms；10 translated page results；immediate artifact total 139,293,175 bytes
  - last completed post-fill-back App profile retained locally：10/10 pages，105,722 ms total，1,195 ms render，20 RWKV requests，0 failed，39,842 RWKV input chars；10 compressed page artifacts total 4,495,692 bytes
  - profile `macos-arm64-pdf2zh`：tag `pdf-layout-pack-macos-arm64-v2026.07.24.1`；395,362,583 bytes；SHA-256 `47f2e41d7c92be4aaaf07583b460ad474fcfc49c367f6c681c67f77e4eccf303`
  - profile `windows-amd64-pdf2zh`：tag `pdf-layout-pack-windows-x64-v2026.07.24.1`；366,073,383 bytes；SHA-256 `10d82633bf08bbac1274ebfdf2ea00d203d1e57267b8b71afc2b6ee10397ea84`
  - profile `linux-x64-pdf2zh`：tag `pdf-layout-pack-linux-x64-v2026.07.15.1`；510,388,352 bytes；SHA-256 `f6492939a7ea919d8d01923f59a78e2c5761abd5428264ca4a636da73dda2034`
- 已确认事实：
  - production source of truth 是 language-scoped `pdf_pages.<targetLang>.json`、committed single-page artifacts 和 assembled export；preview 读取 legacy page state/artifacts，export 在 `rosetta_jobs/mod.rs:3143` 将译文页替换回完整 source PDF
  - production translation handoff 当前仍是 `pdf2zh_invoke.rs:411` 的 `mpsc::unbounded_channel<PdfUnitTranslation>`；该事实留给 CP7，不在 CP0 修改
  - 2026-07-21 用户已对同哈希十页 fixture 完成 full-App visual acceptance；CP0 没有行为变化，因此未要求重复人工验收。任何后续 pack/patch/renderer/request-plan 变化仍由用户负责在同一 fixture 上重新验收
  - 当前本机 Windows pack manifest 标记 `customPack=true`，不得作为 immutable release artifact 或 profile 身份证据；上面的 quality probe 只冻结当前本机代码/engine 行为
  - 本 checkpoint 只修改治理文档，没有修改 renderer、dependency、pack profile、持久化 schema 或用户流程
- 未解决问题或 blocker：
  - CP0 无 blocker
  - CP1 开始前仍需取得被报告约 1.8 GB 的 Linux archive 或构建目录、对应 freeze/build log，以及旧发布 archive；缺任一新 artifact/build directory/freeze 组合时 CP1 必须按停止条件标记 blocked
- 下一步唯一动作：
  - 执行 CP1 Linux 1.8 GB 产物取证；先验证必需输入，再生成 old/new 同格式 inventory 和 freeze diff

#### 2026-07-27 / CP1 / Codex

- 状态：started
- HEAD：`62de5bc`
- worktree baseline：clean
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - `git status --short` -> pass；开始前 worktree clean
  - `git rev-parse --short HEAD` -> `62de5bc`
  - `rg` Linux pack builder/profile/freeze references -> pass；确认 `build-pdf2zh-pack-linux-x64.sh`、`requirements-pdf2zh-linux-x64.txt` 和 2026-07-15 immutable profile 入口仍存在
- 产物：
  - pending
- 已确认事实：
  - CP1 只做 archive、build directory、freeze、build log 和 inventory 取证，不修改 production PDF 行为或 pack 配方
- 未解决问题或 blocker：
  - 正在核对 CP1 必需输入是否可获得
- 下一步唯一动作：
  - 扫描本机候选产物并核对旧 immutable release asset

#### 2026-07-27 / CP1 / Codex

- 状态：blocked
- HEAD：`62de5bc`
- worktree baseline：clean；本 checkpoint 只修改本文状态和 ledger
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - `rg --files -uu C:\Users\Leo` 定向查找 Linux archive、`requirements.freeze.txt` 和 build log -> 无匹配；本机用户目录没有 CP1 新产物输入
  - `gh release view pdf-layout-pack-linux-x64-v2026.07.15.1 --repo LeoLin4258/rosetta-assets` -> pass；旧 immutable release 含 510,388,352-byte archive，release digest 与 profile SHA-256 `f6492939a7ea919d8d01923f59a78e2c5761abd5428264ca4a636da73dda2034` 一致
  - `gh release view pdf-layout-pack-linux-x64-v2026.07.24.1 --repo LeoLin4258/rosetta-assets` -> pass；draft release 仅含 98-byte `.sha256` asset，没有主 archive、freeze 或 build log
  - `gh api repos/LeoLin4258/rosetta-assets/releases/assets/488545124 -H "Accept: application/octet-stream"` -> pass；未发布主 archive 的记录 SHA-256 为 `23060aaba516677346bf1717887c8482145f613de77642b6234ccf544321bb94`
  - `gh run list` 查询 `LeoLin4258/rosetta` 与 `LeoLin4258/rosetta-assets` -> pass；两个仓库均无可下载 Actions artifact
  - 旧 immutable archive 下载探测 -> release 可访问；因新产物仍缺且当前连接吞吐不足，停止并清理 incomplete 临时文件，未把 partial download 作为证据
- 产物：
  - old release metadata：`https://github.com/LeoLin4258/rosetta-assets/releases/tag/pdf-layout-pack-linux-x64-v2026.07.15.1`
  - new draft metadata：`https://github.com/LeoLin4258/rosetta-assets/releases/tag/untagged-853d8488401b372f8a5c`
  - inventory：未生成；缺少 new archive/build directory 和 freeze，不能进行同格式比较
- 已确认事实：
  - 被报告约 1.8 GB 的新主 archive 不在 draft release；仅有 checksum 不能确认 1.8 GB 指 compressed、unpacked 还是 installed runtime
  - 旧 immutable asset 可获得，CP1 的实际阻塞输入只剩新 archive 或构建目录、对应 freeze 和 build log
  - 没有修改 pack builder、dependency、renderer、profile 或 production PDF 行为
- 未解决问题或 blocker：
  - 需要用户提供约 1.8 GB Linux archive 或其构建目录的可访问路径
  - 需要同一次构建产生的 `requirements.freeze.txt` 和完整 build log
- 下一步唯一动作：
  - 取得上述三项输入后恢复 CP1，先确认 1.8 GB 度量类型，再生成 old/new inventory 和 freeze diff

#### 2026-07-27 / CP1 / Codex

- 状态：started
- HEAD：`62de5bc`
- worktree baseline：existing changes: `docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`（前一 CP1 blocked 记录）
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - pinned-host-key SSH probe to `rwkv@192.168.0.115` -> pass；Ubuntu x86_64，remote user/home 为 `rwkv` / `/home/rwkv`
- 产物：
  - remote host key：`ssh-ed25519 255 SHA256:KPoVOiLe0VwRORSjnHHLM2lB2qF8mAX/BsxepEXj6gM`
- 已确认事实：
  - 用户确认该主机是此前测试 Rosetta Linux 版本的机器，可继续检查本机保留的 build evidence
- 未解决问题或 blocker：
  - 尚未定位新产物、freeze 和 build log
- 下一步唯一动作：
  - 只读扫描远程常见构建目录、近期大文件和 release evidence

#### 2026-07-27 / CP1 / Codex

- 状态：blocked
- HEAD：`62de5bc`
- worktree baseline：existing changes: `docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`（本 checkpoint 先前记录）
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`, `rosetta-app/src-tauri/scripts/inventory-pdf2zh-pack.py`, `rosetta-app/src-tauri/scripts/diff-pdf2zh-pack-inventories.py`
- 执行命令与结果：
  - pinned-host-key SSH inventory scan on `rwkv@192.168.0.115` -> pass；定位 2026-07-24 new archive/freeze/manifest、2026-07-15 old archive/freeze/manifest 和 unpacked installed packs
  - `sha256sum` old/new archives -> pass；均与各自 manifest/checksum 一致
  - `inventory-pdf2zh-pack.py` old/new archive scan -> pass；相同算法生成 total/file/symlink、top-level、top 100 directories/files、distribution、model/font/runtime、residue、tests/headers/static libs 和 >=1 MiB duplicate inventory
  - `diff-pdf2zh-pack-inventories.py` inventory/freeze comparison -> pass；目录解释率 100%，site-packages distribution attribution >99.9999%
  - `rg`/`find`/shell history search for 2026-07-24 build log -> 无对应日志；`/tmp/rosetta-linux-pdf-eed557b-*-tauri.log` 是 App/runtime 验证日志，不是 pack build log
  - local Python `compile(...)` and `--help` for both inventory scripts -> pass
  - `python rosetta-app/src-tauri/scripts/test-pdf2zh-patches.py -q` -> pass；34 passed
  - remote `jq empty` on old/new inventory and diff -> pass；JSON 均可解析，重新计算的 SHA-256 与本条产物记录一致
  - `git diff --check` and `git diff --no-index --check` for new scripts -> pass；无 whitespace error，仅有既有 CRLF conversion warning
  - `pnpm typecheck` / `cargo check` / `cargo test rosetta_jobs` -> not-run；本 checkpoint 只新增独立 Python 取证工具和治理文档，没有修改 frontend/Rust/product behavior
- 产物：
  - old archive：`/home/rwkv/src/rosetta-pdfpack-beta22-proxy/rosetta-app/dist/pdf-layout/rosetta-pdf2zh-linux-x64.tar.gz`；510,388,352 bytes；SHA-256 `f6492939a7ea919d8d01923f59a78e2c5761abd5428264ca4a636da73dda2034`
  - new archive：`/home/rwkv/src/rosetta-linux-pdf-workspace-eed557b/rosetta/rosetta-app/dist/pdf-layout/rosetta-pdf2zh-linux-x64.tar.gz`；1,887,604,648 bytes；SHA-256 `23060aaba516677346bf1717887c8482145f613de77642b6234ccf544321bb94`
  - old inventory：`/home/rwkv/src/rosetta-pdfpack-beta22-proxy/rosetta-app/dist/pdf-layout/linux-x64-inventory.json`；SHA-256 `d6d3ead4766037f937ff97a488f92d1d8b045babec629a093e1114da78524d08`
  - new inventory：`/home/rwkv/src/rosetta-linux-pdf-workspace-eed557b/rosetta/rosetta-app/dist/pdf-layout/linux-x64-inventory.json`；SHA-256 `101b07c3d84209356897203c56a6be371c55093892b413ced5664e13a5fdffbd`
  - inventory/freeze diff：`/home/rwkv/src/rosetta-linux-pdf-workspace-eed557b/rosetta/rosetta-app/dist/pdf-layout/linux-x64-inventory-diff.json`；SHA-256 `6c5694d2fc901d6405dcde3a0478237c3f345dd324ea426da36c92c25006185a`
- 已确认事实：
  - “约 1.8 GB”是 compressed archive：old/new 分别为 486.74 MiB / 1,800.16 MiB，增长 269.84%；unpacked 分别为 1,353,005,365 / 3,426,131,261 bytes，增长 153.22%
  - unpacked 净增长 2,073,125,896 bytes；`site-packages` 增长 2,073,231,949 bytes，Python runtime 减少 106,400 bytes，model/font/license 均无变化，因此目录解释率为 100%
  - direct requirements 将 `onnxruntime==1.27.0` 替换为 `onnxruntime-gpu==1.27.0`，并加入 `nvidia-cublas`、`nvidia-cuda-nvrtc`、`nvidia-cuda-runtime`、`nvidia-cudnn-cu13`、`nvidia-cufft`、`nvidia-curand`；freeze 另新增传递依赖 `nvidia-nvjitlink`
  - 最大 distribution 增量为 `nvidia-cublas` 595,369,748、`nvidia-cudnn-cu13` 468,635,308、`onnxruntime-gpu` 305,812,097、`nvidia-cufft` 287,646,467、`nvidia-cuda-nvrtc` 228,019,748 bytes；同时移除 CPU `onnxruntime` 53,062,118 bytes
  - 最大新文件是 `libcublasLt.so.13` 539,533,072 bytes；new archive 的 archive/unpacked ratio 从 old 0.3772 上升到 0.5509
  - cache/build residue 仍为 10,315 bytes，>=1 MiB duplicate bytes 仍为 2,714,697，二者净增长均为 0；tests/headers/static libs 只增长 9,665,136 bytes，不是主要根因
  - new pack 违反 archive、unpacked 和最大单文件 hard cap；regular-file count 为 21,780，未触发 file-count warning
  - build manifest、freeze、archive、checksum 和 runtime logs 存在，但完整 pack build stdout/stderr 没有保存，不能把 runtime log 标成 build log
- 未解决问题或 blocker：
  - CP1 必需输入中的 2026-07-24 build log 缺失；需要找到原日志，或由用户批准把该历史证据缺口记录为例外
- 下一步唯一动作：
  - 取得 build log 或用户例外批准后将 CP1 标记 completed，然后进入 CP2 锁定 Linux pack 输入与构建配方

#### 2026-07-27 / CP1 / Codex

- 状态：completed
- HEAD：`62de5bc`
- worktree baseline：existing changes: CP1 governance and inventory tooling
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - 用户明确批准 2026-07-24 historical build log 缺失例外 -> accepted
- 产物：
  - CP1 old/new archive、inventory、freeze diff 和 budget 证据保持上一条记录
- 已确认事实：
  - CP1 所有 acceptance 已满足；缺失 build log 不再阻塞 checkpoint，但该例外不能用于 CP2/CP11 的新构建，新构建必须保存完整 stdout/stderr
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 开始 CP2，锁定 Linux pack dependency resolution、external input hashes 和 build recipe identity

#### 2026-07-27 / CP2 / Codex

- 状态：started
- HEAD：`62de5bc`
- worktree baseline：existing changes: CP1 governance and inventory tooling
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - CP2 scope/readback and current Linux builder/requirements inspection -> pass
- 产物：
  - pending
- 已确认事实：
  - CP2 选择 `uv pip compile` 作为唯一 lock generator；不引入 pip-tools
  - 当前 main builder 仍使用 unhashed direct requirements，外部 PBS/model/font 输入尚未全部在使用前校验
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 核对 font/PBS/model 下载实现与 Linux `uv` 能力，设计并生成 lock/recipe identity

#### 2026-07-27 / CP2 / Codex

- 状态：completed
- HEAD：`62de5bc`；Linux 验证 worktree 基于远端已有 `8bee070`，所有 CP2 recipe 文件按 SHA-256 单独绑定
- worktree baseline：existing changes: CP1 governance and inventory tooling
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`, `docs/engineering/change-log/2026-07-27-pdf-linux-pack-reproducibility.md`, `rosetta-app/src-tauri/scripts/build-pdf2zh-pack-linux-x64.sh`, `rosetta-app/src-tauri/scripts/compile-pdf2zh-pack-linux-x64-lock.sh`, `rosetta-app/src-tauri/scripts/pdf2zh-linux-x64-inputs.json`, `rosetta-app/src-tauri/scripts/requirements-pdf2zh-linux-x64.txt`, `rosetta-app/src-tauri/scripts/requirements-pdf2zh-linux-x64.lock`, `rosetta-app/src-tauri/scripts/patch-pdf2zh-color-preservation.py`, `rosetta-app/src-tauri/scripts/test-pdf2zh-patches.py`
- 执行命令与结果：
  - pinned `uv 0.11.32` `pip compile --generate-hashes --only-binary :all:` -> pass；生成 104-distribution Linux x64/Python 3.12 lock
  - 两次 isolated Linux build -> pass；均使用相同 PBS、model、font source、PDFMathTranslate commit、lock 和 recipe 文件，完整 stdout/stderr 各保存 51 行
  - 首次构建尝试 -> failed before artifact；固定 `990bed0` 源码使用单行 `source_chars` guard，而 patch 只识别旧多行 guard；补充兼容分支和回归测试后通过
  - 一次重试 -> failed before dependency install；远端 GitHub 443 transient timeout；连接恢复后用相同固定输入重试通过，未将失败尝试计入双构建证据
  - build A/B `requirements.freeze.txt` and `build-recipe.json` `cmp` -> identical
  - build A/B inventory diff -> pass；除 archive path/compressed bytes/SHA/ratio 外，unpacked bytes、file/directory/symlink counts、top-level areas、directories、files、distributions、residue、duplicates、tests/headers/static libraries 全部零差异
  - build A/B in-place and relocated real PDF smoke -> pass；runtime imports 28/28
  - isolated build A `prewarm -> prepareRun -> collectUnits -> identity renderPages` against `2604.17278v1.pdf` -> pass；CP0 unit authority、10-page render 和 artifact bytes 精确匹配
  - `python rosetta-app/src-tauri/scripts/test-pdf2zh-patches.py -q` -> pass；39 passed
  - `bash -n` Linux builder/lock compiler and JSON parse -> pass
- 产物：
  - dependency lock：SHA-256 `c47978def1c4de4c40298f151508fb5f3242fa6bef8e7b00dea98a1f22dcbe15`
  - build recipe ID：`1c51c5de6db135db06890cda3f496e5e17649236462744d8272daf5794fc93fb`
  - build A archive：`/home/rwkv/cp2-dist-a/rosetta-pdf2zh-linux-x64.tar.gz`；510,482,265 bytes；SHA-256 `e2fa35a2c4f3ce1e378052c2a477d3852262b5c21bcbf6be3c72076b4c4cb2e0`
  - build B archive：`/home/rwkv/cp2-dist-b/rosetta-pdf2zh-linux-x64.tar.gz`；510,477,373 bytes；SHA-256 `f1ec05a74751159333402a085379a864812aac33437c0003eb2a7201b8b3bdbb`
  - build A/B freeze：SHA-256 `ad93cc5acae4cb5e6364d2dccc20eb023f1a2273a4a175008ee30925b6947fa5`
  - build A/B inventory：SHA-256 `87a2507d7cddca94058bce2a583dae1b11666d6bf9eb21c964be8c1bd2b92ee6` / `d980f5d39752fb458738dc5756bfe692b723be74a50cd6851f5c4ed7b990bd30`；共同 unpacked 1,354,528,694 bytes、21,792 regular files、5,077 directories、1,048 symlinks
  - reproducibility diff：`/home/rwkv/cp2-dist-b/linux-x64-reproducibility-diff.json`；SHA-256 `beae2d458b17b7bd26577d509ee973387aab35b4e561289f80cc5a0f96aff7e7`
  - build A/B logs：SHA-256 `1a7e57b2c8bb7476faad31a350e87f2c7d33624b166d29b36215116add919a6d` / `3ddaa848160a32cb2513f46b153f0e6c1ec53fb383238162b4f91dda7b70c622`
  - CP0 quality evidence：`/home/rwkv/cp2-dist-a/linux-x64-cp0-quality.json`；SHA-256 `507b7652cb3987e4da3cffe232b65ad01277dc908188f24adad4b53bf04f1b3f`
- 已确认事实：
  - lock 安装使用 `--require-hashes --only-binary=:all: --no-deps`；PDFMathTranslate 固定 Git commit 后使用 pack 内已锁定的 `hatchling` 做 `--no-build-isolation --no-deps` 安装，没有静默源码依赖构建
  - PBS、DocLayout model 和三个 BabelDOC fonts 均在使用前验证固定 SHA-256；manifest 绑定 inputs manifest、lock compiler、builder、requirements、color patch 和 font stager hashes
  - 两次 build 的 archive SHA 不同仅反映尚未规范化的 tar/gzip metadata；CP2 明确只以 freeze 和内容 inventory 一致为 acceptance，archive reproducibility 需要另行固定 tar order、mtime、owner/group 和 gzip timestamp
  - CP0 fixture 保持 94 units、41,035 source chars、unit SHA-256 `81d6185ffc72f263bbc03a6ab1872e4e8615728ad47ecd359b1b2b1d2f3cecb5`、10/10 translated pages 和 139,293,175 artifact bytes
  - 新 archive 与 unpacked size 均在 CP1 预算内；本 checkpoint 未改变 translation-unit authority 或 fill-back 输出
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 执行 CP3，以 import trace 和 reverse dependency 证据逐类评估可删除依赖，不修改 renderer heuristic

#### 2026-07-27 / CP3 / Codex

- 状态：started
- HEAD：`62de5bc`
- worktree baseline：existing CP1/CP2 governance、inventory、lock 与 builder changes；全部保留并继续作为 CP3 基线
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`
- 执行命令与结果：
  - CP3 scope、CP2 acceptance 与当前 Linux builder/lock inspection -> pass
  - Linux test host `rwkv@192.168.0.115` host key `SHA256:KPoVOiLe0VwRORSjnHHLM2lB2qF8mAX/BsxepEXj6gM` -> verified；CP2 A/B archives、fixtures 与 685 GiB free disk available
- 产物：
  - pending
- 已确认事实：
  - CP2 archive 510,482,265 bytes、unpacked 1,354,528,694 bytes，已在 CP1 hard budgets 内；CP3 仍需逐项证明 optional provider 与 runtime residue 的安全删除边界
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 对 CP2 pack 执行 import trace、reverse dependency、distribution/license 与 runtime residue 取证，确定逐类裁剪候选

#### 2026-07-27 / CP3 / Codex

- 状态：completed
- HEAD：`62de5bc`；Linux candidate worktree 基于远端已有 `8bee070`，CP3 builder/SBOM generator 由 build recipe SHA-256 绑定
- worktree baseline：existing CP1/CP2 governance、inventory、lock 与 builder changes；全部保留
- 修改文件：`docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md`, `docs/engineering/change-log/2026-07-27-pdf-linux-pack-reproducibility.md`, `rosetta-app/src-tauri/scripts/build-pdf2zh-pack-linux-x64.sh`, `rosetta-app/src-tauri/scripts/generate-pdf2zh-pack-sbom.py`, `rosetta-app/src-tauri/scripts/pdf2zh-linux-x64-inputs.json`
- 执行命令与结果：
  - `python -X importtime -c 'from pdf2zh import rosetta_engine'` -> pass；Azure、DeepL、Ollama、OpenAI、Tencent、Xinference 全部在 production engine import chain 中加载
  - `pipdeptree 2.29.0 --reverse` -> pass；六类 provider SDK 均由 `pdf2zh` 声明，OpenAI 同时由 BabelDOC 声明；pinned `translator.py` 对六类 SDK 均为 eager import，因此 CP3 未删除或伪造 provider package，解耦留给 CP8
  - OpenCV dependency/link inspection -> `opencv-python` 由 `rapidocr-onnxruntime -> BabelDOC` 引入、headless variant 由 BabelDOC/pdf2zh 引入，当前 `cv2.abi3.so` RPATH 绑定 `opencv_python.libs`；为避免改变 locked resolution 和图像行为，CP3 未删除任一 OpenCV distribution
  - isolated category experiments against CP2 pack -> headers/static libraries、Tcl/Tk/IDLE、79 个 test 目录、非生产 console scripts 分别通过 10-page prepare/collect/identity render；每轮均保持 94 units、41,035 chars、frozen unit hash 和 139,293,175 artifact bytes
  - GNU `strip --strip-debug` experiment -> rejected；会损坏 PBS `python3.12` 并产生 undefined symbol，未进入 builder
  - first two full-build attempts -> stopped before build by GitHub TLS/443 outage；改用已有 partial clone 中已验证的 immutable `990bed0` object 建立 clean detached worktree后，输入身份不变
  - final isolated Linux build -> pass；in-place、relocated、post-prune real PDF smoke 均通过，runtime imports 28/28
  - final extracted candidate 10-page CP0 quality -> pass；94 units、41,035 source chars、10/10 translated pages、unit SHA-256 `81d6185ffc72f263bbc03a6ab1872e4e8615728ad47ecd359b1b2b1d2f3cecb5`、139,293,175 artifact bytes
  - SBOM/license generation -> pass；105 installed distributions、CPython runtime、3 fonts、DocLayout model 与 196 retained license files；font/model licenses 作为 hashed build inputs，declared license files fail closed on missing，manifest sidecar hashes一致
  - `python rosetta-app/src-tauri/scripts/test-pdf2zh-patches.py -q` -> pass；39 passed
  - `pnpm typecheck` -> pass
  - remote `bash -n`、local SBOM script AST parse、JSON parse、`git diff --check` -> pass
- 产物：
  - build recipe ID：`b7f4d6dc71c617c97f377644a2be13ed6c187082f3c014deeb260d81ab323910`
  - archive：`/home/rwkv/cp3-dist/rosetta-pdf2zh-linux-x64.tar.gz`；475,184,227 bytes；SHA-256 `6cd0d56e57b9e2c3fa601789c02486d550b46d1d18e53b0a5b7641fa99215bfa`
  - inventory：`/home/rwkv/cp3-dist/linux-x64-inventory.json`；SHA-256 `0576d1c066de66e0c09a97054216fccf9e1203701505dd3ff96759791fdecace`；unpacked 1,262,340,076 bytes、11,103 regular files、1,081 directories、1,044 symlinks、max file 218,461,128 bytes
  - SBOM：`/home/rwkv/cp3-dist/linux-x64-sbom.cdx.json`；SHA-256 `d96b07dbd4e180750a3816fb1a201718f88c34619caff43dfd9bcc8f7d304f8c`
  - license inventory：`/home/rwkv/cp3-dist/linux-x64-licenses.inventory.json`；SHA-256 `8d125b8ca7554b17ae64c10fd8d2647a000978673dd0847ba7f26812f6d9fc8b`
  - CP0 quality：`/home/rwkv/cp3-dist/linux-x64-cp0-quality.json`；SHA-256 `eb848276867c3daacf019e47157965dae0a4e521d1bbcfc75cec86aecfba2cfa`
  - build log：`/home/rwkv/cp3-dist/linux-x64-build.log`；SHA-256 `16f88be35f96cd999a77b0dc914c8d714436081f5996ecb3d3171d326e7ea34b`
- 已确认事实：
  - candidate 相对 CP2 build A 减少 35,298,038 compressed bytes、92,188,618 unpacked bytes；archive、unpacked、file count、max file 和 symlink 均在 CP1 budget 内
  - builder 每类裁剪记录 byte delta，并在全部裁剪后重新执行 real prepare/collect/render；没有修改 renderer heuristic、translation unit、source payload 或视觉回填逻辑
  - provider SDK 与 duplicate OpenCV 当前不能在 CP3 安全删除；这不阻塞预算和 CP3 acceptance，但 CP8 如解耦 fork imports 必须重新跑全部 CP0/CP3 质量门禁
- 未解决问题或 blocker：
  - none
- 下一步唯一动作：
  - 执行 CP4，设计向后兼容的 engine revision/capabilities 与 fail-closed readiness

#### 2026-07-27 / CP3 user acceptance staging / Codex

- 状态：completed
- Linux App：`/home/rwkv/Applications/Rosetta-0.1.0-beta.22.AppImage`
- live pack：`/home/rwkv/.local/share/com.rosetta.desktop/pdf2zh-sidecar/pack/linux-x64`；custom manifest 绑定 CP3 archive SHA-256 `6cd0d56e57b9e2c3fa601789c02486d550b46d1d18e53b0a5b7641fa99215bfa`
- rollback pack：`/home/rwkv/.local/share/com.rosetta.desktop/pdf2zh-sidecar/pack/linux-x64.before-cp3-20260727`；原 1.8 GB candidate 未删除
- installed-path validation：10-page fixture prepare 4,492.9 ms、94 units、41,035 source chars、unit SHA-256 `81d6185ffc72f263bbc03a6ab1872e4e8615728ad47ecd359b1b2b1d2f3cecb5`、10/10 identity render、139,293,175 artifact bytes
- validation artifact：`/home/rwkv/cp3-installed-validation/quality.json`；SHA-256 `cc0406e7034824d7622210379e8f06d336459a1262ec9dc925e5d4c3a8e7e50f`
- 下一步：用户从现有 AppImage 人工验收 fresh-job 预解析速度与真实中文回填质量；不要删除 rollback pack，直到用户确认

#### 2026-07-28 / CP3 Linux preparse acceptance repair / Codex

- 状态：completed
- 根因：Linux CP3 pack 的 `onnxruntime 1.27.0` 使用自动 Provider 列表 `AzureExecutionProvider + CPUExecutionProvider`，且 `intra_op_num_threads=0` 在 16C/32T EPYC 上产生 32 个 runnable worker；失败样本在 13 秒 wall time 内累计 331.875 CPU-seconds，而同一 cgroup 的 Tauri/WebKit/RWKV 仅累计 2.604 CPU-seconds，确认性能悬崖位于 Linux ORT CPU thread pool，不是 PDF I/O、预览、缓存或 UI 调度
- 修复：Linux worker 启动时将纯 CPU layout session 收敛为 `CPUExecutionProvider`，按当前 affinity 的物理核拓扑显式设置 16 intra-op threads；Windows/macOS GPU provider 路径保持不变；移除无效的每次 cache miss 640px synthetic wakeup
- App：`/home/rwkv/Applications/Rosetta-0.1.0-beta.23-c3b5a8b-ort16.AppImage`；SHA-256 `d86d2163e3b0d917ac80b57b298b1f505ae21a3fc762e965ae6b6131ceb64c08`
- 真实 UI fresh-job：3,847 ms（layout 1,899 ms）和 3,801 ms（layout 1,869 ms），均为 cache miss；此前真实 UI 失败样本为 10,961–12,973 ms（layout 8,058–10,117 ms）
- 质量门禁：94 units、41,035 source chars、canonical unit SHA-256 `81d6185ffc72f263bbc03a6ab1872e4e8615728ad47ecd359b1b2b1d2f3cecb5`，与 CP0/CP3 frozen baseline 完全一致
- 下一步：用户在 `ort16` AppImage 上人工验收冷启动后的预解析体感和真实中文回填；继续保留 prewarm AppImage 与 rollback pack，直到用户确认

#### 2026-07-28 / CP3 Linux user acceptance / Codex

- 状态：completed
- HEAD：`c3b5a8b`
- 用户决定：接受最新真实 UI 10-page fresh-job 预解析 6,280 ms 为“足够接近 5 秒”，CP3 不再以严格 `<5s` 阻塞，也不继续为追逐该数字修改 renderer、unit collection 或 RWKV policy
- 最新任务：`job-1785216764420-2604-17278v1`；preparse cache miss 6,280 ms，其中 layout 4,172 ms、unit collection 1,693 ms，94 units
- 翻译结果：10/10 pages completed、0 failed、0 fallback units、0 empty output、0 truncated output；总计 5,454 ms，其中 RWKV 3,766 ms、render 1,662 ms；用户认为实际中文回填可接受
- PDF 视觉复验：10/10 translated page artifacts 均为可渲染的单页 Letter PDF，图、表、公式和主体双栏结构保留；已记录第 1 页竖排 arXiv 标识/专名、第 8 页跨栏断词 `vi-`、第 9–10 页参考文献专名与跨栏衔接等现有质量缺口，本轮不以修改 renderer heuristic 处理
- 未解决 release issue：artifact compression 0/10 completed；AppImage 注入的 `PYTHONHOME` / `PYTHONPATH` 污染 pack Python，导致 `ModuleNotFoundError: No module named 'encodings'`。原始回填页被安全保留且本次视觉验收不受影响，但 CP11 full-App release gate 前必须修复并复验压缩体积与文本保留
- 范围审计：没有违反本文冻结边界；未改变 renderer、translation-unit authority、RWKV request plan、持久化 schema 或用户流程。Linux ORT Provider/thread policy 修复超出 CP3 dependency-diet 原始任务清单，但属于用户明确要求的 CP3 Linux 验收修复，且 canonical unit/output authority 未变化，已在上一 ledger 条目单独记录
- 下一步唯一动作：执行 CP4，设计向后兼容的 engine revision/capabilities 与 fail-closed readiness；不要在 CP4 顺手修改 renderer 或 artifact compression

#### 2026-07-28 / CP3 verification and diagnostic hardening / Codex

- 状态：completed
- HEAD：`c3b5a8b`；复核对象为当前未提交 CP3 Linux 验收修复 worktree
- 复核结果：CP3 archive `475,184,227` bytes、unpacked `1,262,340,076` bytes、11,103 regular files、1,044 symlinks、max file `218,461,128` bytes；archive SHA-256 `6cd0d56e57b9e2c3fa601789c02486d550b46d1d18e53b0a5b7641fa99215bfa` 与远端 immutable artifact 一致
- 质量证据：远端 CP0 quality 仍为 10 pages、94 units、41,035 source chars、unit SHA-256 `81d6185ffc72f263bbc03a6ab1872e4e8615728ad47ecd359b1b2b1d2f3cecb5`、139,293,175 artifact bytes；inventory、SBOM、license inventory、quality、build log 和 installed validation hashes 均与 CP3 ledger 一致
- 隐私收尾：`ROSETTA_PDF_DIAGNOSTICS` 只接受 `1/true/yes/on`；Rust 不再输出原始 worker JSON，Python prepare diagnostics 不再包含 source/output/scratch/cache path、source fingerprint、raw request 或 normalized options
- 本地验证：`pnpm typecheck` pass；`python src-tauri/scripts/test-pdf2zh-patches.py -q` 39 passed；`cargo check` pass；`cargo test rosetta_jobs` 134 passed；`cargo test managed_pdf2zh::worker` 8 passed；`cargo fmt -- --check` pass；Python worker `py_compile` pass；`git diff --check` pass
- 未解决 release issue：AppImage artifact compression 的 `PYTHONHOME` / `PYTHONPATH` 污染仍归 CP11 full-App release gate，不阻塞 CP4
- 下一步唯一动作：将当前 CP3 验收修复作为独立提交封存，随后执行 CP4；不要把 CP4 改动混入当前 worktree

## 新 agent 接手提示词

```text
接手 Rosetta PDF 稳定化治理。

先阅读：
- AGENTS.md
- docs/engineering/plans/2026-07-27-pdf-stabilization-governance.md
- 当前 checkpoint 指定的代码和脚本

以代码和实际产物为准，不以历史 PDF 文档为准。
运行 git status --short 和 git rev-parse --short HEAD。
只推进 execution ledger 中“下一步唯一动作”指向的一个 checkpoint。
开始和结束都更新本文状态与 ledger；不要新建 handoff/change-log 文档。
不要改变当前已接受的预解析、RWKV request plan 或视觉回填行为，除非 checkpoint 明确授权且用户重新验收。
不要运行 dev server 或 production app build，除非 checkpoint 或用户明确要求 runtime/release verification。
```
