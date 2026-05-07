import type { ChangeEvent } from "react";
import { useRosettaStore } from "../../store/useRosettaStore";
import type { RwkvConnectionConfig, TranslationMode } from "../../types/rosetta";

const modeOptions: Array<{ label: string; value: TranslationMode }> = [
  { label: "极速", value: "fast" },
  { label: "平衡", value: "balanced" },
  { label: "连贯", value: "coherent" },
];

export function SettingsPage() {
  const rwkv = useRosettaStore((state) => state.rwkv);
  const updateRwkvConfig = useRosettaStore((state) => state.updateRwkvConfig);
  const setTranslationMode = useRosettaStore((state) => state.setTranslationMode);

  function updateTextField(field: keyof Pick<RwkvConnectionConfig, "baseUrl">) {
    return (event: ChangeEvent<HTMLInputElement>) => {
      updateRwkvConfig({ [field]: event.currentTarget.value });
    };
  }

  return (
    <section className="mx-auto max-w-3xl px-6 py-6">
      <div className="rounded-lg border border-zinc-800 bg-zinc-900/60 p-5">
        <h2 className="text-base font-semibold text-zinc-50">RWKV 连接</h2>

        <div className="mt-5 grid gap-4">
          <label className="grid gap-2 text-sm">
            <span className="text-zinc-300">API 地址</span>
            <input
              className="h-10 rounded-md border border-zinc-700 bg-zinc-950 px-3 text-zinc-100 outline-none transition-colors placeholder:text-zinc-600 focus:border-emerald-500"
              onChange={updateTextField("baseUrl")}
              value={rwkv.baseUrl}
            />
          </label>

          <label className="grid gap-2 text-sm">
            <span className="text-zinc-300">Batch 端点</span>
            <select
              className="h-10 rounded-md border border-zinc-700 bg-zinc-950 px-3 text-zinc-100 outline-none transition-colors focus:border-emerald-500"
              onChange={(event) =>
                updateRwkvConfig({
                  batchEndpoint: event.currentTarget
                    .value as RwkvConnectionConfig["batchEndpoint"],
                })
              }
              value={rwkv.batchEndpoint}
            >
              <option value="/translate/v1/batch-translate">
                /translate/v1/batch-translate
              </option>
              <option value="/big_batch/completions">/big_batch/completions</option>
            </select>
          </label>

          <div className="grid gap-2 text-sm">
            <span className="text-zinc-300">翻译模式</span>
            <div className="grid grid-cols-3 overflow-hidden rounded-md border border-zinc-700">
              {modeOptions.map((option) => (
                <button
                  className={[
                    "h-10 border-r border-zinc-700 text-sm last:border-r-0",
                    rwkv.mode === option.value
                      ? "bg-emerald-500 text-zinc-950"
                      : "bg-zinc-950 text-zinc-300 hover:bg-zinc-800",
                  ].join(" ")}
                  key={option.value}
                  onClick={() => setTranslationMode(option.value)}
                  type="button"
                >
                  {option.label}
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
