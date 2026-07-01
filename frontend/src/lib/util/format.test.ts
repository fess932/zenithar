import { test, expect, describe } from "bun:test";
import { fmtSize, fmtDur } from "./format";

describe("fmtSize", () => {
  test("bytes below 1 KB", () => expect(fmtSize(512)).toBe("512 B"));
  test("1023 stays in bytes", () => expect(fmtSize(1023)).toBe("1023 B"));
  test("1024 becomes 1 KB", () => expect(fmtSize(1024)).toBe("1 KB"));
  test("KB rounds to a whole number", () => expect(fmtSize(3072)).toBe("3 KB"));
  test("MB keeps one decimal", () => expect(fmtSize(1.5 * 1024 * 1024)).toBe("1.5 MB"));
});

describe("fmtDur", () => {
  test("zero", () => expect(fmtDur(0)).toBe("0:00"));
  test("pads seconds under ten", () => expect(fmtDur(9)).toBe("0:09"));
  test("minutes and seconds", () => expect(fmtDur(65)).toBe("1:05"));
  test("floors fractional seconds", () => expect(fmtDur(59.9)).toBe("0:59"));
  test("ten minutes", () => expect(fmtDur(600)).toBe("10:00"));
});
