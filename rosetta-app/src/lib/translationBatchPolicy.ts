import type { RwkvProviderHandle } from "@/types/rosetta";

const DEFAULT_TEXT_BATCH_SIZE = 16;
const LIGHTNING_TEXT_BATCH_SIZE = 100;

export function textBatchSizeForProvider(provider: RwkvProviderHandle) {
  if (provider.id === "rwkv-lightning-contents") {
    return LIGHTNING_TEXT_BATCH_SIZE;
  }
  return DEFAULT_TEXT_BATCH_SIZE;
}

