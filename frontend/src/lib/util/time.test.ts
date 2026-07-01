import { test, expect, describe } from "bun:test";
import { fmtTime, fullTime, sameDay } from "./time";

// Timestamps built from LOCAL components → tests are timezone-independent.
const at = (y: number, mo: number, d: number, h: number, mi: number, s = 0): number =>
  new Date(y, mo, d, h, mi, s).getTime();

describe("fmtTime", () => {
  test("pads hours and minutes", () => expect(fmtTime(at(2024, 0, 1, 9, 5))).toBe("09:05"));
  test("noon", () => expect(fmtTime(at(2024, 0, 1, 12, 0))).toBe("12:00"));
  test("late evening", () => expect(fmtTime(at(2024, 0, 1, 23, 59))).toBe("23:59"));
});

describe("fullTime", () => {
  test("includes zero-padded seconds", () =>
    expect(fullTime(at(2024, 0, 1, 9, 5, 3))).toBe("09:05:03"));
});

describe("sameDay", () => {
  test("same calendar day, different times", () =>
    expect(sameDay(at(2024, 0, 1, 0, 0), at(2024, 0, 1, 23, 59))).toBe(true));
  test("next day is not the same", () =>
    expect(sameDay(at(2024, 0, 1, 23, 59), at(2024, 0, 2, 0, 1))).toBe(false));
  test("same day-of-month in a different month is not the same", () =>
    expect(sameDay(at(2024, 0, 1, 12, 0), at(2024, 1, 1, 12, 0))).toBe(false));
});
