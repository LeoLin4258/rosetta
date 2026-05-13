import type {
  RwkvConnectionConfig,
  RwkvProviderHandle,
  RwkvProviderId,
} from "@/types/rosetta";

const DEFAULT_MANAGED_RUNTIME_BASE_URL = "http://127.0.0.1:8765";

export type SelectProviderInput = {
  /**
   * Current external API connection config. Used directly when the chosen
   * provider is `rwkv-lightning-contents`; otherwise only `timeoutMs` is
   * borrowed.
   */
  config: Pick<
    RwkvConnectionConfig,
    "baseUrl" | "endpoint" | "internalToken" | "bodyPassword" | "timeoutMs"
  >;
  /**
   * Explicit override that bypasses runtime-status auto-detection. Wired from
   * a future Settings toggle ("use local RWKV" / "use external API"). When
   * unset, falls back to the managed-runtime heuristic below.
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
};

/**
 * Pick the provider handle that the translation runner should dispatch to.
 *
 * Order of precedence:
 *   1. `override` — explicit user/system intent always wins.
 *   2. `managedRuntimeReady` + `managedRuntimeBaseUrl` — local sidecar is up.
 *   3. fall back to `rwkv-lightning-contents` with external API config.
 *
 * Currently the managed-runtime branch only fires when callers explicitly set
 * `managedRuntimeReady` — the runtime status slice that feeds this is wired
 * in Phase 5. Until then this function returns the lightning-contents handle
 * for every existing call site, preserving production behavior verbatim.
 */
export function selectProvider({
  config,
  override,
  managedRuntimeReady,
  managedRuntimeBaseUrl,
}: SelectProviderInput): RwkvProviderHandle {
  const providerId = resolveProviderId(override, managedRuntimeReady);
  if (providerId === "rwkv-mobile-batch-chat") {
    return {
      id: "rwkv-mobile-batch-chat",
      baseUrl: managedRuntimeBaseUrl ?? DEFAULT_MANAGED_RUNTIME_BASE_URL,
      timeoutMs: config.timeoutMs,
    };
  }
  return {
    id: "rwkv-lightning-contents",
    baseUrl: config.baseUrl,
    endpoint: config.endpoint,
    internalToken: config.internalToken,
    bodyPassword: config.bodyPassword,
    timeoutMs: config.timeoutMs,
  };
}

function resolveProviderId(
  override: RwkvProviderId | undefined,
  managedRuntimeReady: boolean | undefined
): RwkvProviderId {
  if (override) {
    return override;
  }
  if (managedRuntimeReady) {
    return "rwkv-mobile-batch-chat";
  }
  return "rwkv-lightning-contents";
}
