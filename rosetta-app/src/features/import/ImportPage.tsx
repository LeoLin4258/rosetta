import { FileText, FolderOpen } from "lucide-react";
import { useRosettaStore } from "../../store/useRosettaStore";

export function ImportPage() {
  const createDemoJob = useRosettaStore((state) => state.createDemoJob);

  return (
    <section className="mx-auto flex max-w-5xl flex-col gap-6 px-6 py-6">
      <div className="grid gap-4 md:grid-cols-[1.4fr_1fr]">
        <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-5">
          <div className="flex items-start justify-between gap-4">
            <div>
              <h2 className="text-base font-semibold text-zinc-50">导入文档</h2>
              <p className="mt-1 text-sm text-zinc-400">TXT、Markdown、基础 DOCX</p>
            </div>
            <FileText className="h-5 w-5 text-zinc-500" />
          </div>

          <div className="mt-5 rounded-lg border border-dashed border-zinc-700 bg-zinc-950 px-5 py-10 text-center">
            <FolderOpen className="mx-auto h-8 w-8 text-zinc-500" />
            <div className="mt-3 text-sm text-zinc-300">拖入文件或选择本地文档</div>
            <button
              className="mt-4 inline-flex h-9 items-center rounded-md bg-emerald-500 px-4 text-sm font-medium text-zinc-950 transition-colors hover:bg-emerald-400"
              type="button"
            >
              选择文件
            </button>
          </div>
        </div>

        <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-5">
          <h2 className="text-base font-semibold text-zinc-50">验证入口</h2>
          <div className="mt-4 space-y-3 text-sm text-zinc-400">
            <div className="flex items-center justify-between border-b border-zinc-800 pb-3">
              <span>RWKV API</span>
              <span className="text-zinc-300">待连接</span>
            </div>
            <div className="flex items-center justify-between border-b border-zinc-800 pb-3">
              <span>翻译模式</span>
              <span className="text-zinc-300">平衡</span>
            </div>
            <div className="flex items-center justify-between">
              <span>任务缓存</span>
              <span className="text-zinc-300">JSON</span>
            </div>
          </div>
          <button
            className="mt-5 h-9 rounded-md border border-zinc-700 px-4 text-sm font-medium text-zinc-200 transition-colors hover:border-zinc-600 hover:bg-zinc-800"
            onClick={createDemoJob}
            type="button"
          >
            新建演示任务
          </button>
        </div>
      </div>
    </section>
  );
}
