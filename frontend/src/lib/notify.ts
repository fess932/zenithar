// Notifications for incoming anonymous-client messages (employees only): a short
// chime, a clickable toast, and a tab-title badge. Unread counting lives in
// chat.ts (`unread`); this layer is the noisy/visible side and respects per-room
// muting. Muting silences sound + toast but NOT the unread badge.
import { writable, get } from "svelte/store";
import {
  onClientNotice,
  onIncoming,
  onReactionNotice,
  unread,
  joinRoom,
  uuid,
  type ClientNotice,
  type ReactionNotice,
} from "./chat";
import { me } from "./session";

export interface Toast {
  id: string;
  room_id: string;
  from_name: string;
  preview: string;
}

export const toasts = writable<Toast[]>([]);

/// A reaction-on-your-message nudge: just an emoji + who, no message preview.
export interface ReactionToast {
  id: string;
  room_id: string;
  from_name: string;
  emoji: string;
}

export const reactionToasts = writable<ReactionToast[]>([]);

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

/// A short chime, synthesized so we ship no audio asset (works offline). `soft`
/// plays a quieter single low blip — for a message in the tab you're already
/// looking at; the default two-note chime is for a backgrounded tab.
function chime(soft = false): void {
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
    const peak = soft ? 0.05 : 0.18;
    const note = (freq: number, start: number, dur: number): void => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, now + start);
      gain.gain.exponentialRampToValueAtTime(peak, now + start + 0.012);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + start + dur);
      osc.connect(gain).connect(ctx.destination);
      osc.start(now + start);
      osc.stop(now + start + dur);
    };
    if (soft) {
      note(587.33, 0, 0.1); // D5 — single soft blip
    } else {
      note(880, 0, 0.12); // A5
      note(1318.5, 0.1, 0.16); // E6
    }
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

// ---- reaction toasts (quiet "someone reacted to your message" nudges) -------

function pushReactionToast(r: ReactionNotice): void {
  const id = uuid();
  reactionToasts.update((list) =>
    [...list, { id, room_id: r.room_id, from_name: r.from_name, emoji: r.emoji }].slice(-MAX_TOASTS),
  );
  setTimeout(() => dismissReactionToast(id), TOAST_MS);
}

export function dismissReactionToast(id: string): void {
  reactionToasts.update((list) => list.filter((x) => x.id !== id));
}

export function openReactionToast(t: ReactionToast): void {
  joinRoom(t.room_id);
  dismissReactionToast(t.id);
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

  // Chime on an incoming message in the room you're viewing: a quiet blip when
  // the tab is foreground (you can see it, just a nudge), the full chime when
  // it's backgrounded. This also gives an anonymous client a sound when an
  // employee answers. Skips your own echoed message and muted rooms.
  onIncoming((m) => {
    if (m.author_id === get(me)?.id) return;
    if (isMuted(m.room_id)) return;
    chime(document.visibilityState === "visible");
  });

  // Someone reacted to your message: a quiet visual nudge only — no chime, no
  // unread badge. It's a light heart, not a message.
  onReactionNotice((r) => pushReactionToast(r));
}
