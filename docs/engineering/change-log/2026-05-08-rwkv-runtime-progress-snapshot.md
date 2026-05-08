# 2026-05-08 RWKV Runtime Progress Snapshot

## 范围

新增 RWKV runtime 模块进度快照：

```txt
docs/engineering/plans/2026-05-08-rwkv-runtime-progress-snapshot.md
```

该文档记录：

- 当前已实现的 Tauri commands
- 本地 app data layout
- runtime status / install plan / install progress / artifact catalog 的完成状态
- manifest 和 artifact 校验规则
- Rust 测试覆盖范围
- RWKV Lightning 与翻译模型的外部调研结论
- Stage 0 前必须确认的 blocker
- 后续阶段状态

## 原因

RWKV runtime 模块已经完成较多 skeleton 和校验逻辑。继续推进前需要明确当前边界，避免在没有真实模型和 runtime 实测数据的情况下继续堆 placeholder。

## 验证

本次为文档更新，未运行代码验证。

按要求未执行：

- `corepack pnpm dev`
- `corepack pnpm build`

