import { test, expect } from "bun:test";
import { formatOf, sticker, STICKERS } from "./stickers";

test("formatOf infers the renderer from the extension", () => {
  expect(formatOf("x.webm")).toBe("webm");
  expect(formatOf("x.webp")).toBe("webp");
  expect(formatOf("x.gif")).toBe("gif");
  expect(formatOf("x.json")).toBe("lottie");
  expect(formatOf("x.lottie")).toBe("lottie");
});

test("sticker() resolves a known id and misses cleanly", () => {
  expect(sticker("heart")?.file).toBe("heart.json");
  expect(sticker("does-not-exist")).toBeUndefined();
});

test("every bundled sticker id is unique", () => {
  const ids = STICKERS.map((s) => s.id);
  expect(new Set(ids).size).toBe(ids.length);
});
