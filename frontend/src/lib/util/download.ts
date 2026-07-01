// Platform-aware download-URL resolution for the "Download app" menu item. Pure +
// framework-free → unit-tested in download.test.ts (bun test, no browser). The
// actual network fetch of the release lives in the caller (Header.svelte).

export type OS = "windows" | "mac" | "android" | "linux" | "other";

export const REPO = "fess932/zenithar";
export const RELEASES_PAGE = `https://github.com/${REPO}/releases/latest`;
export const RELEASE_API = `https://api.github.com/repos/${REPO}/releases/tags/latest`;

export const OS_NAME: Record<OS, string> = {
  windows: "Windows",
  mac: "macOS",
  android: "Android",
  linux: "Linux",
  other: "",
};

/// Best-effort OS from a user-agent string. iOS is NOT treated as mac (no iOS
/// build), even though its UA contains "Mac OS X".
export function detectOS(ua: string): OS {
  if (/android/i.test(ua)) return "android";
  if (/windows/i.test(ua)) return "windows";
  if (/mac/i.test(ua) && !/iphone|ipad|ipod/i.test(ua)) return "mac";
  if (/linux/i.test(ua)) return "linux";
  return "other";
}

export interface ReleaseAsset {
  name: string;
  browser_download_url: string;
}

/// The best release asset URL for an OS, or undefined if none matches (caller
/// then falls back to the releases page). Windows prefers the NSIS setup.exe.
export function pickAsset(os: OS, assets: ReleaseAsset[]): string | undefined {
  const by = (re: RegExp) => assets.find((a) => re.test(a.name))?.browser_download_url;
  switch (os) {
    case "android":
      return by(/\.apk$/i);
    case "windows":
      return by(/setup\.exe$/i) ?? by(/\.exe$/i) ?? by(/\.msi$/i);
    case "mac":
      return by(/\.dmg$/i);
    case "linux":
      return by(/\.AppImage$/i) ?? by(/\.deb$/i);
    default:
      return undefined;
  }
}
