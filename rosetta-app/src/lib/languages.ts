import type { RwkvConnectionConfig } from "@/types/rosetta";

export const LANGUAGE_OPTIONS = [
  { value: "en", label: "英文" },
  { value: "zh-CN", label: "简体中文" },
] as const;

export const SOURCE_LANGUAGE_OPTIONS = LANGUAGE_OPTIONS;

export function languageLabel(value: string): string {
  return LANGUAGE_OPTIONS.find((opt) => opt.value === value)?.label ?? value;
}

export function isRwkvConfigReady(
  config: RwkvConnectionConfig,
  managedRuntimeReady = false
) {
  if (config.providerPreference === "local") {
    return managedRuntimeReady && config.timeoutMs > 0;
  }
  return (
    config.baseUrl.trim().length > 0 &&
    config.endpoint.trim().length > 0 &&
    config.internalToken.trim().length > 0 &&
    config.bodyPassword.trim().length > 0 &&
    config.timeoutMs > 0
  );
}
