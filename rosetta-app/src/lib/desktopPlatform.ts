export type DesktopPlatform = "macos" | "windows" | "linux";

function detectDesktopPlatform(): DesktopPlatform {
  if (typeof navigator === "undefined") {
    return "linux";
  }

  const platform = navigator.platform.toLowerCase();
  if (/mac|iphone|ipad|ipod/.test(platform)) {
    return "macos";
  }
  if (platform.includes("win")) {
    return "windows";
  }

  // Unknown desktop hosts use the opaque Linux treatment. It is the safe
  // fallback when no native translucent window material is configured.
  return "linux";
}

export const desktopPlatform = detectDesktopPlatform();
export const desktopPlatformClass = `rosetta-${desktopPlatform}`;
export const isMacPlatform = desktopPlatform === "macos";
