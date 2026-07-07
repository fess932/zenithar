// Recently-used reaction emoji, most-recent first, persisted so the reaction bar
// can float a person's favourites to the top (Telegram-style).
import { writable } from "svelte/store";

const KEY = "zenithar.recentReactions";
const MAX = 24;

// The default quick set, used before anyone has reacted (and to pad the bar).
export const DEFAULT_REACTIONS = ["👍", "❤️", "😂", "🔥", "🎉", "😮", "😢", "🙏"];

function load(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    const arr = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(arr) ? arr.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export const recentReactions = writable<string[]>(load());

/// Record a used emoji: move it to the front, dedupe, cap the list.
export function pushRecent(emoji: string): void {
  recentReactions.update((list) => {
    const next = [emoji, ...list.filter((e) => e !== emoji)].slice(0, MAX);
    try {
      localStorage.setItem(KEY, JSON.stringify(next));
    } catch {
      /* storage full / disabled — the in-memory list still works this session */
    }
    return next;
  });
}
