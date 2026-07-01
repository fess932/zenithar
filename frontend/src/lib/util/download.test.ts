import { test, expect, describe } from "bun:test";
import { detectOS, pickAsset, type ReleaseAsset } from "./download";

describe("detectOS", () => {
  test("Android", () =>
    expect(detectOS("Mozilla/5.0 (Linux; Android 14; Pixel) Mobile")).toBe("android"));
  test("Windows", () =>
    expect(detectOS("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("windows"));
  test("macOS", () =>
    expect(detectOS("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)")).toBe("mac"));
  test("iPhone is NOT mac (its UA also says 'Mac OS X')", () =>
    expect(detectOS("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X)")).toBe("other"));
  test("Linux", () => expect(detectOS("Mozilla/5.0 (X11; Linux x86_64)")).toBe("linux"));
  test("unknown → other", () => expect(detectOS("something weird")).toBe("other"));
});

describe("pickAsset", () => {
  const assets: ReleaseAsset[] = [
    { name: "Zenithar_0.1.0_x64-setup.exe", browser_download_url: "u/exe" },
    { name: "Zenithar_0.1.0_x64_en-US.msi", browser_download_url: "u/msi" },
    { name: "Zenithar_0.1.0_aarch64.dmg", browser_download_url: "u/dmg" },
    { name: "app-arm64-release.apk", browser_download_url: "u/apk" },
  ];

  test("Windows prefers the NSIS setup.exe", () => expect(pickAsset("windows", assets)).toBe("u/exe"));
  test("Windows falls back to .msi when no .exe", () =>
    expect(pickAsset("windows", assets.filter((a) => !/\.exe$/.test(a.name)))).toBe("u/msi"));
  test("macOS → .dmg", () => expect(pickAsset("mac", assets)).toBe("u/dmg"));
  test("Android → .apk", () => expect(pickAsset("android", assets)).toBe("u/apk"));
  test("no matching asset → undefined (caller uses releases page)", () =>
    expect(pickAsset("mac", [assets[3]])).toBeUndefined());
  test("other → undefined", () => expect(pickAsset("other", assets)).toBeUndefined());
});
