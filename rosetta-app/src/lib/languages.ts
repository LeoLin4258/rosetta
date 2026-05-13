import type { RwkvConnectionConfig } from "@/types/rosetta";

export const LANGUAGE_OPTIONS = [
  { value: "en", label: "English" },
  { value: "zh-CN", label: "简体中文" },
] as const;

export const SOURCE_LANGUAGE_OPTIONS = LANGUAGE_OPTIONS;

export function isRwkvConfigReady(config: RwkvConnectionConfig) {
  return (
    config.baseUrl.trim().length > 0 &&
    config.endpoint.trim().length > 0 &&
    config.internalToken.trim().length > 0 &&
    config.bodyPassword.trim().length > 0 &&
    config.timeoutMs > 0
  );
}
