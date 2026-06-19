// Notifications for incoming anonymous-client messages (employees only): a short
// chime, a clickable toast, and a tab-title badge. Unread counting lives in
// chat.ts (`unread`); this layer is the noisy/visible side and respects per-room
// muting. Muting silences sound + toast but NOT the unread badge.
import { writable, get } from "svelte/store";
import { onClientNotice, unread, joinRoom, uuid, type ClientNotice } from "./chat";

export interface Toast {
  id: string;
  room_id: string;
  from_name: string;
  preview: string;
}

export const toasts = writable<Toast[]>([]);

// ---- per-room mute (persisted) ---------------------------------------------

const MUTE_KEY = "zenithar.muted";

function readMuted(): string[] {
  try {
    const raw = localStorage.getItem(MUTE_KEY);
    const arr = raw ? (JSON.parse(raw) as unknown) : [];
    return Array.isArray(arr) ? arr.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/// Room ids the user has muted. Sound/toasts are suppressed for these; the
/// unread badge still updates (a quiet count).
export const mutedRooms = writable<Set<string>>(new Set(readMuted()));

mutedRooms.subscribe((set) => {
  try {
    localStorage.setItem(MUTE_KEY, JSON.stringify([...set]));
  } catch {
    /* private mode — keep it in memory only */
  }
});

export function isMuted(room_id: string): boolean {
  return get(mutedRooms).has(room_id);
}

export function toggleMute(room_id: string): void {
  mutedRooms.update((set) => {
    const next = new Set(set);
    if (next.has(room_id)) next.delete(room_id);
    else next.add(room_id);
    return next;
  });
}

// ---- sound ------------------------------------------------------------------

type ACtor = typeof AudioContext;
let audioCtx: AudioContext | null = null;

/// A short two-note chime, synthesized so we ship no audio asset (works offline).
function chime(): void {
  try {
    const Ctor: ACtor | undefined =
      window.AudioContext ??
      (window as unknown as { webkitAudioContext?: ACtor }).webkitAudioContext;
    if (!Ctor) return;
    audioCtx ??= new Ctor();
    const ctx = audioCtx;
    // Autoplay policy: the context starts suspended until a user gesture.
    if (ctx.state === "suspended") void ctx.resume();
    const now = ctx.currentTime;
    const note = (freq: number, start: number, dur: number): void => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, now + start);
      gain.gain.exponentialRampToValueAtTime(0.18, now + start + 0.012);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + start + dur);
      osc.connect(gain).connect(ctx.destination);
      osc.start(now + start);
      osc.stop(now + start + dur);
    };
    note(880, 0, 0.12); // A5
    note(1318.5, 0.1, 0.16); // E6
  } catch {
    /* no audio available — visual notification still fires */
  }
}

// ---- toasts -----------------------------------------------------------------

const TOAST_MS = 7000;
const MAX_TOASTS = 3;

function pushToast(n: ClientNotice): void {
  const id = uuid();
  toasts.update((list) =>
    [...list, { id, room_id: n.room_id, from_name: n.from_name, preview: n.preview }].slice(
      -MAX_TOASTS,
    ),
  );
  setTimeout(() => dismissToast(id), TOAST_MS);
}

export function dismissToast(id: string): void {
  toasts.update((list) => list.filter((x) => x.id !== id));
}

/// Open the room a toast points at, then clear it.
export function openToast(toast: Toast): void {
  joinRoom(toast.room_id);
  dismissToast(toast.id);
}

// ---- tab title badge --------------------------------------------------------

function applyTitleBadge(total: number): void {
  // i18n owns the base title; we just prefix/strip a "(N) " counter.
  const base = document.title.replace(/^\(\d+\)\s+/, "");
  document.title = total > 0 ? `(${total}) ${base}` : base;
}

unread.subscribe((u) => {
  const total = Object.values(u).reduce((a, b) => a + b, 0);
  applyTitleBadge(total);
});

// ---- wiring -----------------------------------------------------------------

let inited = false;

/// Register the sound/toast reaction. Call once after the chat connects.
export function initNotifications(): void {
  if (inited) return;
  inited = true;
  onClientNotice((n) => {
    if (isMuted(n.room_id)) return; // quiet: unread badge still updated in chat.ts
    chime();
    pushToast(n);
  });
}
