// App-wide text/UI scale. Everything in the app sizes in `rem`, so scaling the
// root font-size zooms the whole interface proportionally. Persisted; applied on
// load and on every change.
import { writable } from "svelte/store";

export const FONT_SCALES = [1, 1.25, 1.5] as const;
export type FontScale = (typeof FONT_SCALES)[number];

const KEY = "zenithar.fontScale";

function read(): FontScale {
  try {
    const v = Number(localStorage.getItem(KEY));
    return (FONT_SCALES as readonly number[]).includes(v) ? (v as FontScale) : 1;
  } catch {
    return 1;
  }
}

export const fontScale = writable<FontScale>(read());

fontScale.subscribe((s) => {
  try {
    localStorage.setItem(KEY, String(s));
  } catch {
    /* private mode — in-memory only */
  }
  if (typeof document !== "undefined") {
    // % of the browser's base size (usually 16px): 125% → 20px root → all rem ×1.25.
    document.documentElement.style.fontSize = `${s * 100}%`;
  }
});
