# PDF 稳定化、包体与可维护性治理实施计划

## 文档状态

- 状态：Active
- 创建日期：2026-07-27
- 审计窗口：2026-07-17 至当前 `HEAD`；更早代码只在解释该窗口内的设计来源时取证
- 当前阶段：CP0，尚未开始实施
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

CP1 必须建立旧包的 unpacked baseline，之后把 unpacked bytes、file count 和最大单文件预算写回本文。预算调整必须基于 inventory，不允许猜测。

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

- 状态：`not-started`
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

- [ ] 1.8 GB 的度量类型已确认。
- [ ] 至少 95% 的体积可按目录或 distribution 解释。
- [ ] old/new freeze 和 inventory diff 已保存。
- [ ] 没有把推测写成根因。
- [ ] 最终体积预算已写回本文。

### 停止条件

- 1.8 GB artifact、构建目录和 freeze 都不可获得。此时标记 `blocked`，明确请求所需文件，不允许重建一个不同环境的包后声称复现。

---

## CP2：锁定 Linux pack 输入与构建配方

- 状态：`not-started`
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

- [ ] 无未锁定的 Python distribution。
- [ ] 所有下载输入在使用前验证 hash。
- [ ] 两次构建的 freeze 和 inventory 一致。
- [ ] manifest 可以追溯到完整 build recipe。
- [ ] pack smoke 和 CP0 自动化质量基线通过。

### 停止条件

- 某个依赖没有适用的 Linux wheel并触发本地源码构建；必须先记录 toolchain 和产物身份，不能静默接受不可复现 build。

---

## CP3：Linux dependency diet 与安全裁剪

- 状态：`not-started`
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

- [ ] archive 和 unpacked size 均在预算内。
- [ ] 每个删除项有 import/dependency 证据。
- [ ] real prepare/collect/render smoke 通过。
- [ ] CP0 translation-unit authority 未变化。
- [ ] SBOM 和 license inventory 完整。

### 停止条件

- 任何删除改变 unit count、source payload、rendered page result 或视觉结果。
- 需要修改 renderer heuristic 才能继续缩包。

---

## CP4：pack manifest、engine revision 与兼容性能力

- 状态：`not-started`
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

- [ ] 7 月 15 日旧 Linux pack 和新 pack 能被明确区分。
- [ ] 不满足最低 capability 的 pack fail closed，并给出可操作安装提示。
- [ ] 新 manifest 保持旧安装记录可解析。
- [ ] frontend 不成为 component identity authority。

### 停止条件

- 方案要求破坏现有安装目录或强制删除用户已可用 pack，且没有迁移/升级行为。

---

## CP5：下载、解压和磁盘安全边界

- 状态：`not-started`
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

- [ ] oversized download 在超过限制时立即停止并删除 partial。
- [ ] oversized/file-count/path-escape archive 被拒绝。
- [ ] extraction cancel 有自动化测试。
- [ ] 失败不会先删除旧 pack。
- [ ] fresh install 和 upgrade install 通过。

### 停止条件

- 继续调用平台 `tar` 无法实现可靠 quota/cancel。此时应先提出窄化 Rust archive reader 方案，不能用轮询 UI 状态伪装取消。

---

## CP6：Linux CI 与跨平台编译门禁

- 状态：`not-started`
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

- [ ] Linux test target 编译并通过。
- [ ] PR 可以看到主应用基础验证结果。
- [ ] pack workflow 默认不发布、不改 profile。
- [ ] workflow 失败日志不包含文档文本或凭据。

### 停止条件

- CI 需要新增 secret 或发布权限。基础验证可以继续，发布步骤必须暂停并请求用户授权。

---

## CP7：production 译文队列背压与真实内存指标

- 状态：`not-started`
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

- [ ] 不存在 production `unbounded_channel<PdfUnitTranslation>`。
- [ ] slow renderer 测试中 queue 不超过容量。
- [ ] cancel/failure 不死锁。
- [ ] unit count、render order 和 CP0 benchmark request plan 不变。
- [ ] metrics 明确区分或合并 queue 与 map peak。

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

- 状态：`not-started`
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

- [ ] 所有产物身份和 hash 完整。
- [ ] size gate 通过。
- [ ] Linux CI 通过。
- [ ] fresh/upgrade install 通过。
- [ ] 用户视觉验收通过。
- [ ] profile 只在 immutable asset 验证后更新。
- [ ] 回滚步骤经过至少一次 dry run 或静态验证。

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

- 当前 checkpoint：CP1（`not-started`）
- last completed：CP0
- blocked：无
- last verified HEAD：`61ff0ab`
- 下一步唯一动作：执行 CP1，取得约 1.8 GB Linux artifact/build directory、对应 freeze/build log 和旧发布 archive 后做同格式 inventory 取证

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
