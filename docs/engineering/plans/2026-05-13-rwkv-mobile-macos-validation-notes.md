# rwkv-mobile macOS Apple Silicon Phase 0 验证笔记

记录于 2026-05-13。配套实施计划：[`2026-05-13-macos-rwkv-one-click-implementation.md`](2026-05-13-macos-rwkv-one-click-implementation.md)。

## 验证环境

- 机型：Mac mini M4
- macOS：26 (Tahoe)，arm64
- Xcode CLT：已安装，AppleClang 17.0.0
- cmake 4.1.2 / ninja 1.13.1 / Rust 1.95.0

## 构建参数

```bash
cmake .. -DENABLE_WEBRWKV_BACKEND=ON -DENABLE_MLX_BACKEND=OFF \
         -DENABLE_NCNN_BACKEND=OFF -DENABLE_LLAMACPP_BACKEND=OFF \
         -DBUILD_EXAMPLES=ON -DENABLE_SERVER=ON \
         -DCMAKE_BUILD_TYPE=Release \
         -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
         -G Ninja
ninja -j 8
```

- cmake 配置耗时 ~83s（大头是 web_rwkv_ffi cargo 拉依赖；本机走 USTC sparse 镜像）。
- ninja 构建耗时 ~3–5 分钟（M4 8 核 + 已缓存的 cargo 依赖）。
- 产物：`build/examples/rwkv_server`（23MB）。

> 国内构建要点：
> - cargo 配 USTC sparse：`registry = "sparse+https://mirrors.ustc.edu.cn/crates.io-index/"`，并设 `net.git-fetch-with-cli = true`。
> - git 配 Clash 代理：`git config --global http.proxy http://127.0.0.1:7897` / `https.proxy` 同址。
> - 否则 ncnn submodule（即使关掉 NCNN 后端，CMake 也会去拉它的 git）和 web_rwkv 部分 git 依赖会挂死。

## 模型权重

- 实际路径：`mollysama/rwkv-mobile-models` 仓库的 **`WebRWKV/` 子目录**，不是仓库根目录（原计划写错过）。
- 选用：`WebRWKV/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab`
- 实测大小：**1.3 GB**（不是原研究文档说的 ~600MB）。`prefab` 文件除 nf4 权重外还嵌入 WebRWKV 运行时所需的元数据/索引。
- 模型自报：`V7 / num_layer=24 / num_emb=2048 / num_hidden=8192 / num_vocab=65536 / num_head=32`。
- 下载方式：`hf-mirror.com` 对 LFS 重定向不稳，最终走 Clash 代理 + 原站 huggingface.co，57 秒下完（~25 MB/s）。Rosetta 安装器要默认走代理或 ModelScope 镜像；hf-mirror 作为 fallback 不可靠。

## CLI 实测

```bash
./build/examples/rwkv_server \
  --model /Users/leolin/rwkv-test/models/RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab \
  --tokenizer assets/b_rwkv_vocab_v20230424.txt \
  --backend web-rwkv \
  --host 127.0.0.1 --port 8765 \
  --model-name rwkv-translate
```

**坑点**：
- backend 名是 `web-rwkv`（**带连字符**），不是 `webrwkv`，也不是 `web_rwkv`。源码 `src/runtime.cpp:209` 唯一接受这个串。
- 默认 host 是 `0.0.0.0`，**必须显式传 `--host 127.0.0.1`** 以满足隐私要求。
- `--help` 没列 backend 候选名，错的会直接 `Invalid backend name`。Phase 5 安装/启动失败提示要把这点考虑进去。

## 端点验证结果

### `GET /health`

返回 200，<1ms。

### `GET /v1/batch/supported_batch_sizes`

```json
{"model":"rwkv-translate","supported_batch_sizes":[1,2,3,4,5,6,7,8,9,10,11,12]}
```

M4 mini 上支持到 12 路并发批。Rosetta 默认 batch size 取 8 为保守起点，长 segment 自动降到 4。

### `POST /v1/chat/roles`

```json
{"user_role":"English","assistant_role":"Chinese"}
```

返回回显，立即生效。**全局状态**——单 server 进程同一时刻只能服务一个方向。计划里"单运行时单方向"约定确认必要。

### `POST /v1/batch/chat` （8 段英→中）

成功；choices[].index 与请求顺序严格 1:1 对应；所有 conversation 都拿到了中文译文；模型表现合理（含《银河系漫游指南》梗的句子也直译可接受）。

**最关键发现：译文 content 不是纯中文**

每条 `choices[].message.content` 的格式是：

```
<原英文句>

Chinese: <中文译文>
```

实例：

```
The quick brown fox jumps over the lazy dog.

Chinese: 这只快速的棕色狐狸跳过了懒惰的狗。
```

**对 Rosetta 适配器的影响**：

`mobile_batch_chat.rs` 的响应解析必须做后处理：

```rust
// 简化伪代码
fn extract_translation(content: &str) -> Option<String> {
    // 模式：anything \n\nChinese: <translation>
    content
        .split_once("\nChinese:")
        .map(|(_, tail)| tail.trim().to_string())
}
```

或者更稳健：用正则 `(?ms)\n([A-Za-z]+):\s*(.+?)\s*$` 抓 `<lang>: <text>` 末段。

如果方向变成中→英，前缀会是 `English: <...>`。适配器必须按 `assistant_role` 动态切分，**不能硬编码 `Chinese:`**。

Phase 1 单测里要专门覆盖这种格式解析（含原文回显、含多行原文、含响应只有译文无前缀等异常情况）。

### `timings`

每条 conversation 报告：
- `predicted_per_second` ≈ 59.4 tok/s
- `predicted_per_token_ms` ≈ 16.84 ms
- `prompt_per_second` ≈ 65.4 tok/s

batch=8 时这些数值在所有 conversation 之间是相同的，说明是**整个 batch 的共享吞吐统计**，不是每条单独测的。对调度器来说这意味着 batch 内大小越接近越好（计划已包含此策略）。

## Phase 0 退出条件对照

| 条件 | 状态 |
| --- | --- |
| 1.5B nf4 模型在 M1 8GB 上能起来且不 OOM | ⚠️ 本次仅在 M4 mini 验证，需在 M1 8GB 复测 |
| batch chat 端到端跑通，choices index 一一对应 | ✅ |
| 日志默认无敏感文本 | ⚠️ 未做完整日志审计 |
| 基准结果落表 | ⚠️ 仅 batch=8 单点；M1/M2/M3 + 多 segment 长度桶未跑 |

## 未完成的 Phase 0 任务

下列项不阻塞开始 Phase 1（provider adapter 抽象，不依赖运行时验证），但**必须在 Phase 6 之前补完**：

1. **取消测试**：发起 batch 后 SIGINT / 关 HTTP 连接，观察进程是否清理，状态机能否恢复。
2. **日志审计**：默认日志级别下完整跑一遍翻译，grep stdout/stderr/任何日志文件确认**不出现** prompt 文本和译文。当前 server 启动日志只看到 `ModelInfo`，倾向于干净，但需正式验证。
3. **基准矩阵**：M1 8GB、M3 16GB 至少各一台，扫描 segment 长度桶 100/300/600/1000/1500/2000 字符 × batch size 1/2/4/8/12，落 CSV。决定 Rosetta scheduler 的桶切分阈值。

## 其他需要写进 Phase 1/2 的发现

1. **构建依赖比预期重**：完整 cmake configure 隐式拉了 ncnn 及其 glslang submodule（即使关 NCNN 后端），加上 web_rwkv 的整个 Rust crate 树。Phase 2 的 macOS CI workflow 要预设 `git config --global http.proxy` 或在 CI runner 上不走代理的情况下确保 GitHub 访问稳定。
2. **`--help` 不列 backend 候选名**：上游可以加一行。或者 Rosetta sidecar manifest 自己锁死 `--backend web-rwkv`，不让用户配。
3. **HF 主站走 Clash > hf-mirror 走 LFS**：hf-mirror 在 LFS 重定向上表现不稳，**Rosetta 默认模型源应该是 ModelScope**（已有 mirror 偏好策略），HuggingFace 作为 fallback。
4. **prefab 文件实际 1.3GB**：实施计划 / UI 文案的"~600MB"要改成 **"~1.3 GB"**。

## Server 进程

验证用 server 当前仍在 `127.0.0.1:8765` 监听（手动启动，不是 daemon）。后续任何端到端测试可以直接打这个端口。停止：`pkill rwkv_server`。
