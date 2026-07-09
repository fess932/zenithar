// Bounds how many sticker/thumbnail images load at once. Opening a pack otherwise
// either floods the network (all at once) or drips them one-by-one; this keeps a
// steady handful in flight. Each <img> asks for a slot before it sets its src,
// and frees it once the image finishes (load OR error), letting the next queued
// one start — so images arrive in batches of MAX.
const MAX = 5;

export interface LoadSlot {
  start: () => void;
  state: "wait" | "run" | "done";
}

let active = 0;
const waiting: LoadSlot[] = [];

function pump(): void {
  while (active < MAX) {
    const s = waiting.shift();
    if (!s) break;
    if (s.state !== "wait") continue;
    s.state = "run";
    active++;
    s.start();
  }
}

/// Request a load slot. `start` runs when granted (immediately if under the cap,
/// else when a slot frees). Returns a handle to release later.
export function acquireLoad(start: () => void): LoadSlot {
  const s: LoadSlot = { start, state: "wait" };
  waiting.push(s);
  pump();
  return s;
}

/// Free a slot (image loaded/errored, or the tile was destroyed before its turn).
export function releaseLoad(s: LoadSlot | null): void {
  if (!s || s.state === "done") return;
  if (s.state === "run") active = Math.max(0, active - 1);
  else {
    const i = waiting.indexOf(s);
    if (i >= 0) waiting.splice(i, 1);
  }
  s.state = "done";
  pump();
}
