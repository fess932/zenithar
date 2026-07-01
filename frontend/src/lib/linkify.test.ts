import { test, expect, describe } from "bun:test";
import { linkify } from "./linkify";

describe("linkify", () => {
  test("plain text → single text part, no link", () => {
    expect(linkify("hello world")).toEqual([{ text: "hello world" }]);
  });

  test("empty string → no parts", () => {
    expect(linkify("")).toEqual([]);
  });

  test("a bare http(s) url becomes one link part", () => {
    expect(linkify("https://example.com/a?b=1")).toEqual([
      { text: "https://example.com/a?b=1", href: "https://example.com/a?b=1" },
    ]);
  });

  test("url surrounded by text splits into three parts", () => {
    expect(linkify("see https://a.com now")).toEqual([
      { text: "see " },
      { text: "https://a.com", href: "https://a.com" },
      { text: " now" },
    ]);
  });

  test("trailing sentence punctuation stays out of the link", () => {
    expect(linkify("go to https://a.com.")).toEqual([
      { text: "go to " },
      { text: "https://a.com", href: "https://a.com" },
      { text: "." },
    ]);
  });

  test("a bare www. link gets an https href", () => {
    expect(linkify("www.a.com")).toEqual([{ text: "www.a.com", href: "https://www.a.com" }]);
  });

  test("multiple urls are all linked", () => {
    const parts = linkify("a http://x.com b https://y.com c");
    expect(parts.filter((p) => p.href).map((p) => p.href)).toEqual([
      "http://x.com",
      "https://y.com",
    ]);
  });

  test("plain text is never given an href (XSS-safe segmentation)", () => {
    const parts = linkify("<img onerror=alert(1)> not a url");
    expect(parts.every((p) => p.href === undefined)).toBe(true);
  });

  test("concatenating part texts reproduces the input exactly", () => {
    const input = "start https://a.com/x), mid www.b.org! end";
    expect(linkify(input).map((p) => p.text).join("")).toBe(input);
  });
});
