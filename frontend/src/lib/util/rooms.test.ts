import { test, expect, describe } from "bun:test";
import { roomLabel, roomPreview } from "./rooms";

describe("roomLabel", () => {
  test("common uses the provided (localized) label", () =>
    expect(roomLabel({ kind: "common" }, "Общий")).toBe("Общий"));
  test("direct becomes a @handle", () =>
    expect(roomLabel({ kind: "direct", title: "Ivan" }, "Общий")).toBe("@Ivan"));
  test("direct without a title", () =>
    expect(roomLabel({ kind: "direct", title: null }, "x")).toBe("@?"));
  test("client uses its title", () =>
    expect(roomLabel({ kind: "client", title: "Acme" }, "x")).toBe("Acme"));
  test("client without a title", () => expect(roomLabel({ kind: "client" }, "x")).toBe("—"));
});

describe("roomPreview", () => {
  test("group rooms prefix the sender", () =>
    expect(roomPreview({ kind: "common", last_author: "Ivan", last_body: "hi" })).toBe("Ivan: hi"));
  test("a 1:1 direct room has no sender prefix", () =>
    expect(roomPreview({ kind: "direct", last_author: "Ivan", last_body: "hi" })).toBe("hi"));
  test("empty body → clip marker (attachment-only)", () =>
    expect(roomPreview({ kind: "client", last_author: "Ivan", last_body: "" })).toBe("Ivan: 📎"));
  test("no author and no body", () => expect(roomPreview({ kind: "common" })).toBe("📎"));
});
