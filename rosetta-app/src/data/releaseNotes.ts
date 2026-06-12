/**
 * Per-version highlights shown in Settings → 关于.
 *
 * **What goes here**: short user-facing bullets summarizing what a version
 * brought. Aim for 3–6 bullets, each ≤ 1 line wrapped. Engineering details
 * belong in `docs/engineering/change-log/`, not here — these notes are for
 * end users deciding "do I want to upgrade" or remembering "what did I gain
 * from the last upgrade".
 *
 * **When to add an entry**: every time the app version bumps in
 * `package.json` / `Cargo.toml` / `tauri.conf.json`. Add the entry **in the
 * same commit** as the version bump — otherwise the Settings page will show
 * an empty card for the new version.
 *
 * **Why we keep this in-app instead of fetching from network**: the updater
 * plugin already supplies the *new* version's notes via `update.body`. What
 * the user is missing without this file is the *current* version's notes —
 * which they should see even offline, even when no update is available.
 * Bundling avoids a second network round-trip and "checking update" gets
 * to render both sides instantly.
 *
 * Order: newest first. Iteration helpers use `Object.keys` so insertion
 * order is preserved.
 */

export type ReleaseNote = {
  version: string;
  /** Short user-facing bullets. Plain text; no markdown rendering. */
  highlights: string[];
};

/**
 * Lookup `Record<version, ReleaseNote>`. Versions are the exact strings
 * appearing in `package.json` (e.g. `"0.1.0-beta.8"`), not semver tuples.
 */
export const RELEASE_NOTES: Record<string, ReleaseNote> = {
  "0.1.0-beta.11": {
    version: "0.1.0-beta.11",
    highlights: [
      "空状态页新增新建文件入口，可输入文件名创建空白 TXT 文档",
      "TXT 原文区域支持进入编辑模式，保存后会重新按段落切分原文",
      "新建 TXT 文件沿用导入 TXT 的翻译流程，可直接选择段落并开始翻译",
      "原文编辑保存后会清空旧译文状态，避免新原文显示旧译文",
      "空状态页入口简化为导入文件、导入文件夹、新建文件",
    ],
  },
  "0.1.0-beta.9": {
    version: "0.1.0-beta.9",
    highlights: [
      "设置页重新整理层级，翻译引擎、外观、文档处理和关于信息更清晰",
      "本地模型和 PDF 组件管理面板改为更紧凑的折叠布局",
      "新增一键清除本机数据：任务历史、本地模型、PDF 组件和本机设置可一次移除",
      "清除本机数据时会保留原始文件、手动导出的文件和 Rosetta 应用本身",
    ],
  },
  "0.1.0-beta.8": {
    version: "0.1.0-beta.8",
    highlights: [
      "本地翻译模型从 1.5B WebRWKV 换成 0.4B MLX，体积 ~360 MB（小 ¾），M-系列 Mac 上更快",
      "升级时自动清理旧版 1.26 GB 模型，无需手动操作",
      "PDF 翻译并发并行度提升到与 markdown 一致，单页翻译速度显著提升",
      "PDF 翻译进度新增「第 X/Y 页 · 00:23 · N%」实时显示，长时间任务不再像卡死",
      "修复多个 PDF 翻译可能卡在「翻译中」的问题（含系统代理拦截 loopback 的兼容问题）",
      "PDF 版面处理组件支持从本地文件导入安装（大陆用户可绕过 GitHub 下载）",
    ],
  },
  "0.1.0-beta.7": {
    version: "0.1.0-beta.7",
    highlights: [
      "PDF 逐页翻译能力上线（按页选择、单页失败可独立重试）",
      "PDF 双语对照导出",
      "Markdown 翻译稳定性改进",
    ],
  },
};

/**
 * Return the release note for `version`, or `null` if we don't have one
 * (typical when running a dev build whose version string isn't in this
 * file yet — Settings should fall back to a "no notes available" message
 * rather than crash).
 */
export function getReleaseNote(version: string): ReleaseNote | null {
  return RELEASE_NOTES[version] ?? null;
}
