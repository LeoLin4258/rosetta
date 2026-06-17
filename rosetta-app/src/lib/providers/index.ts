import type {
  RwkvConnectionConfig,
  RwkvProviderHandle,
  RwkvProviderId,
  RwkvProviderPreference,
} from "@/types/rosetta";

export type SelectProviderInput = {
  /**
   * Current external API connection config. Used directly when the chosen
   * provider is `rwkv-lightning-contents`; otherwise only `timeoutMs` is
   * borrowed.
   */
  config: Pick<
    RwkvConnectionConfig,
    | "baseUrl"
    | "endpoint"
    | "internalToken"
    | "bodyPassword"
    | "timeoutMs"
    | "providerPreference"
  >;
  /**
   * Explicit override that bypasses the saved Settings preference. Mostly used
   * by call sites that already mapped a user-facing choice to a provider id.
   */
  override?: RwkvProviderId;
  /**
   * Whether the managed local sidecar is currently ready (model loaded,
   * `/health` 200). Phase 5 will populate this from the managed runtime
   * status slice; Phase 1 callers leave it `false` so behavior is identical
   * to today.
   */
  managedRuntimeReady?: boolean;
  /**
   * `127.0.0.1:<ephemeral-port>` URL of the running managed sidecar. Phase 5
   * passes the live value; Phase 1 defaults to the Phase 0 validation port
   * (`8765`) so manual end-to-end checks against a hand-started server work
   * without extra wiring.
   */
  managedRuntimeBaseUrl?: string;
  managedRuntimeProviderId?: RwkvProviderId;
};

/**
 * Pick the provider handle that the translation runner should dispatch to.
 *
 * Order of precedence:
 * Order of precedence:
 *   1. `override` — explicit call-site intent always wins.
 *   2. `config.providerPreference` — user-selected Settings value.
 *   3. `managedRuntimeReady` — compatibility fallback for older persisted data.
 *   4. remote API.
 */
export function selectProvider({
  config,
  override,
  managedRuntimeReady,
  managedRuntimeBaseUrl,
  managedRuntimeProviderId,
}: SelectProviderInput): RwkvProviderHandle {
  const providerId = resolveProviderId(
    override,
    config.providerPreference,
    managedRuntimeReady,
    managedRuntimeProviderId
  );
  if (config.providerPreference === "local") {
    if (!managedRuntimeReady || !managedRuntimeBaseUrl) {
      throw new Error("本地 RWKV 运行时尚未就绪，请先在设置中启动或修复本地翻译模型。");
    }
  }
  if (providerId === "rwkv-mobile-batch-chat" && !managedRuntimeBaseUrl) {
    throw new Error("本地 RWKV 运行时尚未提供可用地址，请先启动本地翻译模型。");
  }
  if (providerId === "rwkv-mobile-batch-chat") {
    return {
      id: "rwkv-mobile-batch-chat",
      baseUrl: managedRuntimeBaseUrl!,
      timeoutMs: config.timeoutMs,
    };
  }
  return {
    id: "rwkv-lightning-contents",
    baseUrl: providerId === "rwkv-lightning-contents" && config.providerPreference === "local"
      ? managedRuntimeBaseUrl!
      : config.baseUrl,
    endpoint: config.providerPreference === "local" ? "/v1/batch/completions" : config.endpoint,
    internalToken: config.providerPreference === "local" ? "" : config.internalToken,
    bodyPassword: config.providerPreference === "local" ? "" : config.bodyPassword,
    timeoutMs: config.timeoutMs,
  };
}

function resolveProviderId(
  override: RwkvProviderId | undefined,
  preference: RwkvProviderPreference | undefined,
  managedRuntimeReady: boolean | undefined,
  managedRuntimeProviderId: RwkvProviderId | undefined
): RwkvProviderId {
  if (override) {
    return override;
  }
  if (preference === "local") {
    return managedRuntimeProviderId ?? "rwkv-mobile-batch-chat";
  }
  if (preference === "remote-api") {
    return "rwkv-lightning-contents";
  }
  if (managedRuntimeReady) {
    return managedRuntimeProviderId ?? "rwkv-mobile-batch-chat";
  }
  return "rwkv-lightning-contents";
}
