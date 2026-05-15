# RWKV Translate 模型短输入续写问题（上游待回复）

> 状态：draft / upstream-investigating
> 日期：2026-05-15
> 模型：`RWKV_v7_G1c_1.5B_Translate_ctx4096_20260118-nf4.prefab`（WebRWKV / macOS arm64）
> 相关：[2026-05-13-rwkv-mobile-macos-validation-notes.md](2026-05-13-rwkv-mobile-macos-validation-notes.md)

## 上游进展

- **2026-05-15**：联系 [@MollySophia](https://github.com/MollySophia)，附上"安全"短词 ramble 的复现 curl 和响应。Molly 回复："去复现一下（可能是后端也可能是模型问题）"。等她诊断结论。客户端不动。

## 现象

RWKV translate 模型对**短输入**（CJK 1–2 字 / 英文 1–2 词）会"展开成词典条目"而不是给出纯译文。

复现：

```bash
curl -X POST http://127.0.0.1:65352/v1/batch/chat \
  -H 'Content-Type: application/json' \
  -d '{"conversations":[{"messages":[{"role":"user","content":"范围"}]}],"max_tokens":1024}'
```

返回 `choices[0].message.content`：
```
范围

English: Range: The range of a function is the set of all possible output values that can be obtained when the input value varies within a certain interval.
```

期望译文：`Range` 或 `Scope`。

## 已验证（2026-05-15）

### 请求参数路径全部失效

`/v1/batch/chat` sidecar 当前实现**只认 `max_tokens`**，其它字段静默忽略：

| 尝试的字段 | 结果 |
|---|---|
| `stop_tokens: ["\n\n", ":", "."]` | 输出不变（35 tokens 一字不差）|
| `temperature: 0` | 输出不变 |
| `top_k: 1` | 输出不变 |
| `unknown_field_test: "x"` | 输出不变（佐证：完全没 schema 校验）|

`max_tokens` 也不是甜区方案 —— 设小会切到半句（"Range: The range of a function is the set of all" 这种），设大会 ramble。

### 现象与输入长度强相关

- **长句**（>30 字符）：完美。`finish_reason=stop` 自然结束，无续写。
- **短词**：100% ramble。
- **`conversations[]` 并行 batch**：每条独立 ramble，不互相提供上下文。

### 发现一个客户端可行的 workaround（暂不实施）

把若干短段在**同一 `content`** 里用 `\n` 拼接，模型进入"列表翻译"模式：

| Input | Output | 评价 |
|---|---|---|
| `"范围"` | `"Range: The range of a function is..."` | ❌ |
| `"范围\n范围"` | `"Range, Scope"` | ✅ |
| `"范围\n测试\n开发"` | `"Range: Test, Development"` | ✅（接近）|
| `"测试开发\n\n范围"` | `"Test Development Scope"` | ✅ |
| `"产品功能列表如下：\n范围"` | `"Product Function List:"` | ❌（漏译"范围"）|

但**伪上下文句不可靠**（最后一行），所以这条路是"合短段列表"而不是"加假上下文"。

## 当前决定：暂不实施客户端 workaround

理由：

1. **本质是上游模型 / sidecar 问题**。在客户端做合批拼接是 patch over a patch，会沉淀成长期债。一旦上游修了，删除 workaround 还要做兼容回归。
2. **客户端 workaround 有非平凡的对位风险**。模型有时用 `\n`、有时用 `, `、有时用 `: ` 拆分译文，切回原 segment 不稳。`numbered list` 方案需要单独验证模型是否稳定按格式回。
3. **不影响长文档可用性**。Rosetta 主要场景是文档翻译，绝大多数 block 是完整句，短输入只在标题、列表项、术语表里出现。当前可接受 "短词翻译会出怪文本" 作为已知 bug。
4. 已联系上游维护者 [@MollySophia](https://github.com/MollySophia)，等回复后再决定走向。

## 已联系上游（草稿见聊天记录）

询问的问题：
1. 短输入续写是 translate 模型的已知行为吗？官方推荐 workaround？
2. `/v1/batch/chat` 短期 / 长期会不会暴露 `stop_tokens` / `temperature` / `top_k`？
3. README_translate.md 里展示的响应是干净译文（`{"content":"Hello, world!"}`），实测却是 `原文\n\n{assistant_role}: 译文` 含原文回显。这是文档还是 server 的问题？
4. 我们打算的"短段合批 numbered list"思路有更好的替代吗？
5. nf4 量化是否加剧短输入 ramble？有更高精度（fp16/int8）的桌面模型可放出吗？

## 如果上游不修，未来的实施路径

按 ROI 排序，**等真有用户报怨再做**：

### A. 短段合批（首选）

- 检测短段（< 8 CJK 字符 / < 3 英文词）
- 把**同一 block 内连续短 segment** 合成 `numbered list` 格式：
  ```
  1. 范围
  2. 测试
  3. 开发
  ```
- 期望模型输出 `1. Range\n2. Test\n3. Development`，按编号回填
- 风险：跨 block 合并会破坏版式；模型未必稳定按编号回（要先 Postman 实测 10+ 组样本）
- 回退：单条翻译 + 后处理裁剪

### B. 响应后处理兜底

- 检测 `X: <X 的定义>` 模式硬切第一句
- 长度比异常告警（zh→en 译文长度 > 原文 × 8 视为异常）
- 不可靠，会误伤合理的展开译法

### C. 推上游加 `/v1/translate` 专用端点

- 同公司，可直接沟通（[[project-rwkv-mobile-same-company]]）
- 让 sidecar 内置 `stop_tokens` / `temperature` 默认值，客户端只关心翻译这件事
- 长期最干净，短期不可控

### 不做的方向

- **加 `stop_tokens` 字段** —— 实测被忽略，无效
- **`max_tokens` 动态估算** —— 砍半句反而更糟
- **温度 / top_k 调参** —— 实测被忽略
- **加伪上下文句** —— 实测会导致漏译

## 退出标准

任一条件触发，结束此 plan：

- 上游 sidecar 暴露 `stop_tokens` 或 `temperature` 参数 → 改请求 body，删除此 plan
- 上游模型修复短输入续写 → 验证后删除此 plan
- 用户报怨集中爆发，且上游无近期修复计划 → 落地 A 方案，转 ADR
