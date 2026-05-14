import { invoke } from "@tauri-apps/api/core";
import type {
  CompleteOnboardingRequest,
  OnboardingDecision,
} from "@/types/rosetta";

/**
 * Returns the latest onboarding decision (completed flag + model presence +
 * derived `needsOnboarding`). Called by Rust setup hook to pick which window
 * to show; the frontend uses it to decide branching too when it needs a
 * fresh read (rare — the window label already says which path you're on).
 */
export function getOnboardingDecision() {
  return invoke<OnboardingDecision>("get_onboarding_decision");
}

/**
 * Mark onboarding complete, persist state, and atomically swap windows:
 * show `main` first, then close `onboarding`. Used by:
 *
 * - Done step's "翻译我的第一个文档" CTA after local install succeeded.
 * - Welcome step's "use external API" link (with `skippedLocalInstall: true`).
 */
export function completeOnboardingAndOpenMain(request: CompleteOnboardingRequest) {
  return invoke<OnboardingDecision>("complete_onboarding_and_open_main", {
    request,
  });
}

/**
 * Re-open the onboarding window from elsewhere (e.g. a future "Repair" entry
 * point in Settings). Unused in P1 happy path but cheap to expose.
 */
export function reopenOnboardingWindow() {
  return invoke<void>("reopen_onboarding_window");
}
