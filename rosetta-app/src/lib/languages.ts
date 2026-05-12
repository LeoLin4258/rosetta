import type { RwkvConnectionConfig } from "@/types/rosetta";

export const LANGUAGE_OPTIONS = [
  { value: "zh-CN", label: "简体中文" },
  { value: "zh-TW", label: "繁體中文" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
  { value: "es", label: "Español" },
  { value: "ru", label: "Русский" },
  { value: "pt", label: "Português" },
  { value: "it", label: "Italiano" },
  { value: "vi", label: "Tiếng Việt" },
  { value: "id", label: "Bahasa Indonesia" },
] as const;

export const SOURCE_LANGUAGE_OPTIONS = [
  { value: "en", label: "English" },
  ...LANGUAGE_OPTIONS,
] as const;

export function isRwkvConfigReady(config: RwkvConnectionConfig) {
  return (
    config.baseUrl.trim().length > 0 &&
    config.endpoint.trim().length > 0 &&
    config.internalToken.trim().length > 0 &&
    config.bodyPassword.trim().length > 0 &&
    config.timeoutMs > 0
  );
}
