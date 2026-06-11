# 2026-06-10 MLX 后端切换 + PDF 翻译链路修复

## Context

[`docs/engineering/plans/2026-06-10-mlx-backend-switch.md`](../plans/2026-06-10-mlx-backend-switch.md) 提出把 macOS Apple Silicon 的本地翻译后端从 WebRWKV (1.5B nf4) 换到 MLX (0.4B 6-bit)：体积更小、Apple Silicon 上更快、翻译质量足够。Phase 0 验证由 rwkv-mobile 作者完成。

Rosetta 这一侧按计划改了 `profile.rs` / `layout.rs` / `install.rs` / `lifecycle.rs` / `status.rs` 以及 CI workflow，但实际部署到一台开发机上联调时连续踩出 5 个互相独立的坑——任何一个不修都会让"切换后无法翻译"，且现象高度类似（`/health` 超时 / 翻译卡死），调试不便。这篇文档把每个坑的根因、症状、修复一次性归档，避免下次升级 sidecar 或换后端时重复踩。

## Changes

按真实暴露顺序记录，每条都给"症状 → 根因 → 修复 → 怎么判断这次有没有再次出现"。

### 1. Sidecar 二进制必须重新构建，并同步重新 stage

**症状**：profile.rs 把 `backend` 切到 `mlx` 后，UI 上"启动本地翻译"45 秒内 `/health` 不响应，但应用本身、模型下载、解压都正常。

**根因**：profile.rs 和 `.github/workflows/build-rwkv-sidecar-macos.yml` 的修改**只**改变了源码和 CI 编译参数，**不会**自动重建本地 `rosetta-app/src-tauri/binaries/rwkv-server-aarch64-apple-darwin`。这台机器上跑的是上一轮 WebRWKV 时构建的二进制，被传入 `--backend mlx` 后无法初始化对应后端。

判定证据：`runtime.log` 当天的新行仍出现 `[web_rwkv_ffi] ModelInfo {...}`，纯 MLX 构建里不会有 `web_rwkv_ffi` 这个 logger。

**修复**：升级 sidecar 时**必须**手动跑一次 fetch 脚本——不存在自动重建。两条路径：

```bash
# 路径 A：本地构建（开发期）
git -C ~/Documents/GitHub/rwkv-mobile checkout 9c0780d4eeeb71ff8d5b6b8a0e8588f843427cbf
cd ~/Documents/GitHub/rwkv-mobile
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_OSX_DEPLOYMENT_TARGET=13.0 \
  -DENABLE_WEBRWKV_BACKEND=OFF -DENABLE_MLX_BACKEND=ON \
  -DENABLE_NCNN_BACKEND=OFF -DENABLE_LLAMACPP_BACKEND=OFF \
  -DENABLE_SERVER=ON -DBUILD_EXAMPLES=ON \
  -DHTTPLIB_USE_OPENSSL_IF_AVAILABLE=OFF -DHTTPLIB_REQUIRE_OPENSSL=OFF
ninja -C build rwkv_server
bash rosetta-app/src-tauri/scripts/fetch-rwkv-sidecar.sh --local ~/Documents/GitHub/rwkv-mobile

# 路径 B：跑 CI 然后下载 release
# 推 sidecar-vX.Y.Z tag，GH Actions 跑完后：
bash rosetta-app/src-tauri/scripts/fetch-rwkv-sidecar.sh --tag sidecar-vX.Y.Z
```

**怎么判断**：启动应用后，`tail "$HOME/Library/Application Support/com.rosetta.desktop/managed-rwkv/logs/runtime.log"` 看不到 `web_rwkv_ffi` 字符串，且能正常翻译一段文本。

### 2. MLX 后端需要 `default.metallib` 与二进制同目录

**症状**：sidecar 已经重建为 MLX 版后，启动仍然 `/health` 超时；rwkv-server 进程一启动就退出。

**根因**：rwkv-mobile 的 MLX 后端运行时会 mmap `default.metallib`（预编译的 Metal kernel 库），从**进程工作目录**或**二进制同目录**里找。`build-rwkv-sidecar-macos.yml` 已经把它打进 tarball，但：

- `fetch-rwkv-sidecar.sh` 的旧版 `install_files()` 只装 sidecar + tokenizer，**没**装 metallib；
- `tauri.macos.conf.json` 的 `resources` glob 用的是 `resources/rwkv-sidecar/*`，只要文件落到那个目录就会被 bundle，但 metallib 此前没被 stage 到任何地方；
- 即便 bundle 里有 metallib，签名后的 `.app` 内 `Contents/MacOS/` 通常只读，无法把 metallib copy 进去。

**修复**：分三层：

- `scripts/fetch-rwkv-sidecar.sh`：`install_files()` 与 `--local` 路径都把 `default.metallib` 同时 stage 到 `binaries/` 和 `resources/rwkv-sidecar/`。dev 模式下 `binaries/default.metallib` 与 sidecar 同目录直接可用；bundle 模式下 `resources/rwkv-sidecar/default.metallib` 进 `.app/Contents/Resources/...`。
- `managed_rwkv/status.rs::locate_metallib(...)` 探测三处：sidecar 同目录、bundle 的 Resources 路径、dev 的 `binaries/`。
- `managed_rwkv/lifecycle.rs::start_sidecar(...)`：spawn 前如果 sidecar 同目录没有 metallib，best-effort copy 一份过去；copy 失败（签名 `.app` 只读）则 fallback 把 `command.current_dir(...)` 设到 metallib 所在目录，依赖 MLX 读 cwd-relative 路径的行为。

`managed_rwkv/status.rs::build_install_plan(...)` 新增 `InstallItemKind::Metallib`，metallib 不在时 UI 的 Install Plan 会直接报缺失。

**怎么判断**：第一次启动应用的运行时日志里会出现 `[rwkv-lifecycle] staged default.metallib at ...`；之后启动因为已经在原位，不再 copy，但不报错。

### 3. zip 模型解压目标必须是 `model_extracted_dir`，且要剥离 zip 内的顶层目录前缀

**症状**：模型下载成功、SHA256 校验通过，UI 设置面板"模型路径"还显示 `.zip` 文件路径；尝试启动翻译时 sidecar 报模型路径不存在。

**根因**：MLX 模型是个 zip，解压后是个目录。`layout.rs::RuntimeLayout::model_extracted_dir` 定义为 `model_dir/<zip stem>`，但旧版 `install.rs::extract_zip(zip, dest_dir = layout.model_dir)` 是按 zip 内 entry 名直接解压到 `model_dir`。两种情况：

- zip 内带顶层目录 `<stem>/...` → 解压后变成 `model_dir/<stem>/...`，恰好等于 `model_extracted_dir`，能用。
- zip 内是"flat" 结构（文件直接在根）→ 解压后是 `model_dir/weights.safetensors` 等，`model_extracted_dir` 永远不存在，`mod.rs` 给 sidecar 的 `--model` 路径不存在。

实际 HuggingFace 上这个 zip 的内部结构是 flat，于是踩中第二种。

**修复**：`install.rs::extract_zip` 改为：

1. 调用方传 `dest_dir = layout.model_extracted_dir`（不再传 `model_dir`）。
2. 先扫一遍所有 entry，检测是否存在共同顶层目录前缀。
3. 解压时如有共同前缀就剥掉；没有就保持原样。

无论 zip 是 flat 还是带顶层目录，磁盘上落到 `model_extracted_dir` 是确定的。

附带：`status.rs::ManagedRuntimePaths.model_file` 在 `model_extracted_dir.exists()` 时显示解压目录，否则 fallback 到 zip 路径，避免设置面板继续展示已被 `install.rs` 删除的 zip。

**怎么判断**：首次安装后 UI 设置面板"模型路径"显示的是目录而不是 `.zip` 文件；磁盘上 `~/Library/Application Support/com.rosetta.desktop/managed-rwkv/models/rwkv7-0.4b-mlx-6bit/rwkv7-0.4B-...-mlx-6bit/` 这一层目录存在且内含权重文件。

### 4. pdf2zh 的 `--thread` 必须独立 cap，不能跟 RWKV 的 `supported_batch_sizes` 走

**症状**：sidecar 跑起来了，markdown 翻译正常，但 PDF 翻译"翻译中"无限卡。runtime.log 显示 `Switching from batch size N to M, active batches: 0 1 ... 13` 这种 12-16 个并发批次的状态，PDF job 输出目录里 `rosetta-pdf2zh-command.log` 写着 `threads=16`，pdf2zh 子进程开着，但 `shim.log` 只有 `spawn shim`、没有任何 `request messages=...`。

**根因**：MLX 后端真实暴露了 `/v1/batch/supported_batch_sizes` 端点（WebRWKV 时代该端点不可用），返回 `[1..16]`。`managed_pdf2zh/openai_shim.rs::spawn_shim` 调用 `query_supported_batch_sizes(...) → pick_batch_size(..., hint=0)` 得到 `max_batch_size = 16`。`OpenAiShim.batch_size = 16`，pdf2zh_invoke.rs 把 `shim.batch_size` 直接当作 pdf2zh 的 `--thread` 参数，pdf2zh.py 用 Python multiprocessing 起 16 个 worker。

单页 PDF 上 16 个 worker 起 pool 的固定开销 + resource_tracker 同步本身就能让它在还没发出第一个 OpenAI 请求之前先 hang 住（在线程数明显多于实际并行需求时是常见的 multiprocessing 病理）。WebRWKV 时代 `query_supported_batch_sizes` 失败、走 `DEFAULT_MAX_BATCH_SIZE = 4` 兜底，所以恰好没踩到。

**修复**：把 shim 的内部攒批能力（RWKV 上限）和 pdf2zh 的子进程并发度解耦：

```rust
// openai_shim.rs
const PDF2ZH_THREAD_CEILING: usize = 16;

pub struct OpenAiShim {
    pub batch_size: usize,             // 仍跟 RWKV 上限
    pub pdf2zh_thread_count: usize,    // = batch_size.min(PDF2ZH_THREAD_CEILING)
    ...
}
```

`pdf2zh_invoke.rs` 改为 `let thread_count = shim.pdf2zh_thread_count;`。

**值的演进**：

- `4` —— 最初在 2026-06-10 切换 MLX 后定的保守值。当时 `threads=16` 看起来会卡死小 PDF，归因为"Python multiprocessing pool 起 16 worker 太多"。
- `8` —— 同日提速时的中间档。
- `16` —— 同日继续提速后的终值。**回头复盘发现**：最初 `threads=16` 的 hang 其实是 #5 的代理 bug 在作怪（pdf2zh 的所有 OpenAI 请求都 502），跟 multiprocessing pool 无关。代理修了 + live.log 上线之后 16 完全稳定，M4 mini 上 batch 真能打到 16 满载，跟 markdown 在 MLX 上一样的批大小。

**怎么判断**：dev 终端有 `[pdf2zh-batch] assembled 16 item(s) in batch`；新一次 PDF 任务的 `rosetta-pdf2zh-command.log` 里 `threads=16`。

**剩余的速度差**：即便拉到 16，PDF 仍比 markdown 慢，因为 PDF 链路多出来的固定开销跟 cap 无关：pdf2zh.py 启动（~2-5s）、multiprocessing fork（~3-8s，跟 worker 数正相关）、pdfminer layout 解析（每页 ~0.5-2s）、HTTP loopback + shim 80ms 攒批窗口。小 PDF 上这部分占比很高、感觉相对慢；大 PDF 上摊薄了就接近 markdown。继续压缩要动 pdf2zh.py 这一层（常驻 worker pool / 跨页复用进程），工程量大。

### 5. pdf2zh 子进程必须显式 `NO_PROXY` + 清空 `HTTP(S)_PROXY`

**症状**：修完 #4 后 pdf2zh 终于开始干活，但 `rosetta-pdf2zh-live.log` 持续刷 `ERROR:pdf2zh.converter:Error code: 502`；shim.log **仍然**只有 `spawn shim`，没有 `request messages=...`。即"pdf2zh 在发请求、但请求没到 shim"。

**根因**：pdf2zh.py 用 OpenAI Python SDK（基于 httpx）发请求，SDK 默认认 `HTTP_PROXY` / `HTTPS_PROXY` 环境变量。开发者机器上跑着 Clash/Surge 这类系统代理，shell env 里有 `HTTP_PROXY=http://127.0.0.1:7890` 之类的设置。pnpm tauri dev → rosetta-app → pdf2zh 子进程逐层继承，pdf2zh 把发往 `http://127.0.0.1:<shim_port>/v1/chat/completions` 的请求也路由给了系统代理，代理尝试把这个请求当外网请求处理，连不上自己，回 502 Bad Gateway。

为什么 markdown 不踩：markdown 走 Rust 端 `mobile_batch_chat::translate_batch`，里面的 `loopback_client()` 显式 `.no_proxy()`，从来不读 env。PDF 这条链多了 pdf2zh 子进程 + Python SDK 两层，它们都会无脑认 env。

**修复**：在 `pdf2zh_invoke.rs` 给子进程显式注入：

```rust
.env("NO_PROXY", "127.0.0.1,localhost,::1")
.env("no_proxy", "127.0.0.1,localhost,::1")
.env("HTTP_PROXY", "")
.env("HTTPS_PROXY", "")
.env("ALL_PROXY", "")
.env("http_proxy", "")
.env("https_proxy", "")
.env("all_proxy", "")
```

`NO_PROXY` 双写大小写：Python 的 urllib 只看小写，httpx 看大写。同时把所有代理 env 显式置空，因为 httpx 的某些版本会忽略 NO_PROXY 对 loopback 的特例处理，宁可 belt + suspenders。

**怎么判断**：live.log 不再出现 `Error code: 502`，dev 终端能看到 `[pdf2zh-batch] assembled N item(s)` 与 `[pdf2zh-batch] result: ok=true, translations=N`，shim.log 出现 `translation_preview=...` 行。

### 6. 调试用：pdf2zh 子进程 stdout/stderr 实时 tee 到 `rosetta-pdf2zh-live.log`

**为什么补这个**：调试 #4 / #5 时 pdf2zh 在 hang 而不是 fail，旧逻辑只在 `!status.success()` 时把内存里最后 30 行写盘成 `rosetta-pdf2zh-output.log`，所以 hang 死的情况下完全看不到 pdf2zh.py 在干什么。

**修复**：`pdf2zh_invoke.rs` 在 spawn 之后立刻开一个 `rosetta-pdf2zh-live.log` 文件，stderr/stdout 两个 reader task 各自把 `[stderr] <line>` / `[stdout] <line>` 实时 append。开销可忽略，但 hang 调试体感天差地别。

`rosetta-pdf2zh-output.log` 保留原行为（失败时的 tail），两者并存。

## Verification

每个修复点上面都写了"怎么判断"。整链路一遍跑通的最小校验：

1. `~/Library/Application Support/com.rosetta.desktop/managed-rwkv/logs/runtime.log` 里不出现 `web_rwkv_ffi`。
2. 设置面板"模型路径"显示解压目录而不是 `.zip`。
3. UI 翻译一段 markdown 能返回译文（验证基础链路 + #1 #2 #3）。
4. UI 翻译一个新建的 1–2 页 PDF 能返回译文（验证 #4 #5）。
5. PDF 任务目录 `pdf2zh-output/<page>/rosetta-pdf2zh-command.log` 里 `threads=4`。
6. dev 终端有 `[pdf2zh-batch] result: ok=true, translations=N` 输出。

## Future-proofing

以后升级 sidecar / 切后端 / 引入新的子进程翻译器（pdf2zh 之外的什么 docx2zh、epub2zh）时务必检查这套坑位：

- **二进制要重新 stage**：`fetch-rwkv-sidecar.sh` 是手动动作，CI / profile.rs 改完不会自动跑。在 release checklist 里把"跑一次 fetch 脚本"列为必做。
- **后端依赖的运行时附件要进 bundle + 同目录**：MLX 是 `default.metallib`；以后假如换 libtorch、CoreML、QNN 等后端，每个都会有类似的"必须与二进制同目录" runtime asset。`status.rs::build_install_plan` 是这类附件的"是否就绪"的统一检查点，新增后端时一并扩展 `InstallItemKind`。
- **zip 模型的解压目标必须是 layout 算出的确定路径**：不要依赖 zip 内部目录结构，`extract_zip` 已经做了"自动剥离顶层前缀"的兜底，新增 zip profile 时直接复用，不要在 `extract_zip` 之外手写解压逻辑。
- **任何子进程翻译器都必须显式 `NO_PROXY` + 清 `HTTP(S)_PROXY`**：默认认为子进程会继承不该认的代理 env。在 [`conventions/`](../conventions/) 单独写一个"子进程翻译器接入约定"时把这条列为必做。
- **shim/子进程的并发参数不要直接复用 RWKV 上报的 `supported_batch_sizes`**：RWKV 服务端能消化的并发 ≠ 子进程能稳定起的 worker 数。`PDF2ZH_THREAD_CEILING` 留作显式上限旋钮。

## Changed files

- `rosetta-app/src-tauri/src/managed_rwkv/install.rs` —— `extract_zip` 重写，加 `detect_common_zip_prefix`；call site 改用 `model_extracted_dir`。
- `rosetta-app/src-tauri/src/managed_rwkv/layout.rs` —— 早期改动，`RuntimeLayout.model_extracted_dir` 字段（plan 已记录）。
- `rosetta-app/src-tauri/src/managed_rwkv/lifecycle.rs` —— `start_sidecar` 新增 `metallib_source` 参数；MLX backend 启动前 ensure metallib 与 sidecar 同目录或 fallback 设 cwd。
- `rosetta-app/src-tauri/src/managed_rwkv/status.rs` —— `locate_metallib`、`StaticStatus.metallib_path`、`InstallItemKind::Metallib`、`build_install_plan` 增加 metallib 检测、`ManagedRuntimePaths.model_file` 优先显示解压目录。
- `rosetta-app/src-tauri/src/managed_rwkv/mod.rs` —— 把 `metallib_path` 透传给 `start_sidecar`。
- `rosetta-app/src-tauri/src/managed_pdf2zh/openai_shim.rs` —— `OpenAiShim.pdf2zh_thread_count` 字段 + `PDF2ZH_THREAD_CEILING` 常量；`batch_size` 字段加 `#[allow(dead_code)]`。
- `rosetta-app/src-tauri/src/rosetta_jobs/formats/pdf/pdf2zh_invoke.rs` —— 子进程 env 清代理 + 注入 `NO_PROXY`；`thread_count` 改用 `shim.pdf2zh_thread_count`；新增 `rosetta-pdf2zh-live.log` 实时日志。
- `rosetta-app/src-tauri/scripts/fetch-rwkv-sidecar.sh` —— `install_files()` 与 `--local` 路径同时 stage `default.metallib` 到 `binaries/` 和 `resources/rwkv-sidecar/`。
- `.github/workflows/build-rwkv-sidecar-macos.yml` —— 此前已由 plan 改 commit pin 与 CMake flags，本次未再改。

## Upgrade impact (beta.7 → beta.8)

老用户从 beta.7（WebRWKV 1.5B）升到 beta.8（MLX 0.4B）会遇到这几件事：

### 自动会发生的事

- **旧 1.26 GB 的 WebRWKV 模型自动删除**。`managed_rwkv::migrate::run_migrations` 在 `lib.rs` 的 `setup` 里跑一次，扫 `<app-local-data>/managed-rwkv/models/rwkv-translate-1.5b-nf4/` 这条已知路径并整个干掉；删了多少 MB 会写到 stderr。完全幂等，已经升过的不会再清。
- **旧 sidecar 进程在新启动时不会被复用**。`lifecycle.rs::cleanup_stale_sidecars` 会按当前 profile 的 sidecar 名匹配并 kill 掉残留；既然 beta.8 替换了同名的 `rwkv-server-aarch64-apple-darwin` 二进制为 MLX 构建，旧 PID 会被识别并清理。
- **旧 logs / runtime-state 文件保留**。`runtime.log` 和 `runtime-state/active-runtime.json` 不动；下次启动 sidecar 会 overwrite，不影响新流程。

### 需要用户自己做的事

- **重新下载 360 MB 的 MLX 模型**。第一次启动 beta.8 设置面板会显示"翻译模型未下载"并给出下载按钮（install plan 复用现有 UI，不需要额外引导）。下载的是 zip，应用会自动解压到 `models/rwkv7-0.4b-mlx-6bit/<stem>/`。

### 已有 jobs 的兼容性

PDF / Markdown 翻译任务、缓存的译文、segments.json、pdf_page_translations.json 这些**完全不受影响**——它们跟具体后端无关，只关心译文产物本身。老的已翻译页面继续显示译文 PDF；未翻译的页面新调用走 MLX。

### 一次升级要改的版本号

- `rosetta-app/package.json`：`"version": "0.1.0-beta.8"`
- `rosetta-app/src-tauri/Cargo.toml`：`version = "0.1.0-beta.8"`
- `rosetta-app/src-tauri/tauri.conf.json`：`"version": "0.1.0-beta.8"`

Cargo.lock 自动在下次 build 时刷新，无需手动改。

### 加新 legacy 清理项的方式

未来再有"换后端导致旧文件没用"的场景，往 `managed_rwkv::migrate::LEGACY_ARTIFACTS` 数组里 append 一条 `LegacyArtifact { subpath, reason }` 即可。**不要删除已有条目** —— 越级升级（比如 beta.6 → beta.9）的用户仍然需要它们。

### 升级路径走查发现的 3 个坑（已修，但留下经验）

走查 beta.7 → beta.8 实际用户路径时挖出来的，每个都会破坏"丝滑"。下次换 profile / 换模型类型时再核一遍：

**坑 A：`onboarding::decide` 用 `model_file.is_file()` 判断模型存在性 —— zip profile 上永远 false**

MLX 是 zip profile，安装后 `model_file`（zip 路径）被删了，模型变成 `model_extracted_dir/<stem>/`。原来的检查会让所有升级用户即便下完新模型也被无限送回 onboarding。

修复：在 `layout::RuntimeLayout` 上加 `is_model_installed()` 方法（zip 看 extracted_dir，否则看 model_file），让 onboarding / install / status 三处共用同一个判断。

**Future-proof 约定**：**不要在 `onboarding::decide`、`install_inner` 已安装短路、`build_install_plan` 之外的地方手写 model-presence 检查**。要查就调 `layout.is_model_installed()`。这条要写进 conventions/。

**坑 B：`WelcomeStep` 把"约 1.3 GB"写死在 JSX 里**

升 beta.8 时用户看到一个跟实际下载量（360MB）差 4 倍的数字。

修复：`OnboardingDecision` 加 `model_size_bytes` 字段，从当前 profile 透出；WelcomeStep 加 `formatModelSize()` 显示真值。

**Future-proof 约定**：**用户文案里出现的任何"体积 / 时长 / 模型名"都不要硬编码**，全部从 profile 透出。下次换模型只改 profile.rs，UI 自动跟上。

**坑 C：升级用户看到的是新用户的欢迎屏**

beta.7 用户升完后 onboarding 打开，看到大大的"Rosetta"标题 + "在本机翻译文档"，跟首次安装一模一样，会以为升级把状态搞丢了。

修复：`OnboardingDecision` 加 `is_returning_user`（= `state.completed`，独立于 `model_installed`）。WelcomeStep 根据它切换成"欢迎回来 + 新版本换了更小的模型"。

**Future-proof 约定**：**onboarding 窗口里的所有"首次使用"措辞都必须配 returning user 分支**。理论上 onboarding 只该在两种情况出现 —— 新用户、升级后模型没了的老用户。后者不该被当成前者。

### PDF 版面处理组件的"本地文件导入"兜底

**背景**：pdf2zh pack（281 MB）只放在 `LeoLin4258/rosetta-assets` 的 GitHub Release，没有国内镜像（跟 RWKV 模型不同——后者有 hf-mirror 兜底）。大陆裸网用户基本下不下来。

**设计**：复用现有的 `Pdf2zhInstallOptions::pack_url` 字段（早期为 dogfood 留的口子），它已经支持 `file://...` URL，install 流水线里 `copy_file_url(...)` 跟 `download_http(...)` 是平行分支，**SHA256 校验、体积校验、解压、写 manifest 这些后续步骤全部共用**。所以：

- **后端：零改动。**
- **前端：加一个文件选择器 + 把路径包成 `file://` 传给 install 命令。**

**实现位置**：

- `useManagedPdf2zhRuntime.importFromFile()` —— 调 `@tauri-apps/plugin-dialog` 的 `open()` 拿到绝对路径，转成 `file://...` 调 `install({ repair: true, packUrl })`。`repair: true` 是关键——保证之前失败的下载残留先被清掉，免得用户带着脏状态走二次导入。
- `PdfSetupStep`（onboarding）和 `Pdf2zhPanel`（设置）各加一个"已下载？从本地文件导入"按钮，款式选 ghost / 二级链接，不抢主 CTA 视线。

**Future-proof 约定**：**任何用户需要从网络拿的大文件，UI 都该配一个"本地文件导入"兜底**。包括以后给 RWKV 模型也加一个（虽然 hf-mirror 解决了 95%，但还是有 5% 全死）。后端 install pipeline 只要遵守"`file://` 走 `copy_file_url`，HTTP 走 `download_http`，校验环节统一"这个分支，前端那一个文件选择器就能复用。

**Tauri 配置注意**：`capabilities/default.json` 必须加 `dialog:default` 和 `dialog:allow-open`，否则 plugin-dialog 在主窗口拿不到权限，picker 不会弹。这条非常容易漏，bug 表现是"按钮点了没反应"，要去 devtools console 才能看到 `IPC: not allowed by capability` 之类的错。

## Cross-references

- Plan：[`plans/2026-06-10-mlx-backend-switch.md`](../plans/2026-06-10-mlx-backend-switch.md)
- 上一轮 sidecar 构建管线：[`change-log/2026-05-13-rwkv-sidecar-build-pipeline.md`](2026-05-13-rwkv-sidecar-build-pipeline.md)
- ADR：[`decisions/0003-macos-first-managed-rwkv-runtime.md`](../decisions/0003-macos-first-managed-rwkv-runtime.md)
