import type {
  ManagedRuntimeProfileStatus,
  ManagedRuntimeStatus,
} from "@/types/rosetta";

export const WINDOWS_LIGHTNING_PROFILE_ID =
  "windows-amd64-rwkv-lightning-cuda";
export const WINDOWS_LLAMACPP_PROFILE_ID = "windows-amd64-llamacpp-vulkan";
export const MACOS_MLX_PROFILE_ID = "macos-arm64-mlx";

export function selectManagedRuntimeProfileStatus(
  status: ManagedRuntimeStatus | null,
  selectedProfileId?: string | null
): ManagedRuntimeProfileStatus | null {
  const profileStatuses = status?.profileStatuses ?? [];
  if (profileStatuses.length === 0) {
    return null;
  }

  const selected = selectedProfileId
    ? profileStatuses.find((entry) => entry.profile.id === selectedProfileId)
    : null;
  if (selected) {
    return selected;
  }

  return defaultManagedRuntimeProfileStatus(profileStatuses);
}

export function defaultManagedRuntimeProfileId(
  status: ManagedRuntimeStatus | null
): string | null {
  return selectManagedRuntimeProfileStatus(status)?.profile.id ?? null;
}

export function isManagedRuntimeProfileReady(
  status: ManagedRuntimeProfileStatus | null
): boolean {
  return status?.state === "ready" && !!status.process.baseUrl;
}

function defaultManagedRuntimeProfileStatus(
  profileStatuses: ManagedRuntimeProfileStatus[]
): ManagedRuntimeProfileStatus | null {
  const lightning = findProfile(profileStatuses, WINDOWS_LIGHTNING_PROFILE_ID);
  if (lightning?.hardware.supported) {
    return lightning;
  }

  return (
    findProfile(profileStatuses, MACOS_MLX_PROFILE_ID) ??
    findProfile(profileStatuses, WINDOWS_LLAMACPP_PROFILE_ID) ??
    profileStatuses.find((entry) => entry.state !== "unsupported") ??
    profileStatuses[0] ??
    null
  );
}

function findProfile(
  profileStatuses: ManagedRuntimeProfileStatus[],
  profileId: string
) {
  return profileStatuses.find((entry) => entry.profile.id === profileId) ?? null;
}
