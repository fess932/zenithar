// Recently-used picker items — emoji, stickers, GIFs, saved — floated to the top
// of each picker tab so you don't have to hunt for the ones you actually use
// (Telegram-style). Persisted in localStorage; most-recent first.
import { writable, derived } from "svelte/store";

export type RecentCat = "emoji" | "stickers" | "gifs" | "saved";

export type RecentEntry =
  | { cat: "emoji"; kind: "emoji"; v: string }
  | { cat: "stickers"; kind: "bundled"; id: string }
  | { cat: RecentCat; kind: "item"; id: string; ct: string };

const KEY = "zenithar.recents";
const CAP = 64; // total across categories; each tab slices its own head

function keyOf(e: RecentEntry): string {
  return e.kind === "emoji" ? `e:${e.v}` : e.kind === "bundled" ? `b:${e.id}` : `i:${e.id}`;
}

function load(): RecentEntry[] {
  try {
    const raw = localStorage.getItem(KEY);
    const arr = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(arr) ? (arr as RecentEntry[]).filter((e) => e && "kind" in e) : [];
  } catch {
    return [];
  }
}

export const recents = writable<RecentEntry[]>(load());

function push(entry: RecentEntry): void {
  recents.update((list) => {
    const k = keyOf(entry);
    const next = [entry, ...list.filter((e) => keyOf(e) !== k)].slice(0, CAP);
    try {
      localStorage.setItem(KEY, JSON.stringify(next));
    } catch {
      /* storage full / disabled — in-memory list still works this session */
    }
    return next;
  });
}

export function pushRecentEmoji(v: string): void {
  push({ cat: "emoji", kind: "emoji", v });
}
export function pushRecentBundled(id: string): void {
  push({ cat: "stickers", kind: "bundled", id });
}
export function pushRecentItem(cat: RecentCat, id: string, ct: string): void {
  push({ cat, kind: "item", id, ct });
}

/// The recent entries for one tab (most-recent first, already deduped).
export function recentsFor(cat: RecentCat) {
  return derived(recents, ($r) => $r.filter((e) => e.cat === cat));
}
