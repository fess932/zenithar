// An in-app media viewer (lightbox) with gallery navigation. Opening any image
// or video attachment gathers every image AND video in the current transcript
// into an ordered gallery so you can page through them (arrows / swipe) without
// leaving the app — videos play inline with native controls.
import { get, writable } from "svelte/store";
import { messages } from "./chat";

export interface LightboxItem {
  id: string;
  kind: "image" | "video";
  src: string; // what the viewer shows (a downscaled preview for images)
  download?: string; // full-resolution original for the Download button (default: src)
  alt: string;
  filename: string;
  saveable?: boolean; // show the "save to сохранёнки" button (default true)
}

interface LightboxState {
  items: LightboxItem[];
  index: number;
}

export const lightbox = writable<LightboxState | null>(null);

// History integration: opening the viewer pushes a history entry so the Android
// back gesture / edge-swipe (and Esc, via UI) closes the viewer and returns to
// the chat instead of navigating the app underneath. Mirrors the room nav.
let historyPushed = false;
if (typeof window !== "undefined") {
  window.addEventListener("popstate", () => {
    // Our entry was popped (back gesture) → close, without pushing back again.
    if (historyPushed && !(history.state && (history.state as { lb?: boolean }).lb)) {
      historyPushed = false;
      lightbox.set(null);
    }
  });
}
function pushHistory(): void {
  if (typeof history !== "undefined" && !historyPushed) {
    history.pushState({ lb: true }, "");
    historyPushed = true;
  }
}

const orig = (id: string) => `/api/attachments/${id}`;
const preview = (id: string) => `/api/attachments/${id}/preview`;

/// Open the viewer at the given attachment, with all transcript images and
/// videos as the gallery (so prev/next can page through them).
export function openLightbox(attachmentId: string): void {
  const items: LightboxItem[] = [];
  for (const m of get(messages)) {
    for (const a of m.attachments) {
      const kind = a.content_type.startsWith("image/")
        ? "image"
        : a.content_type.startsWith("video/")
          ? "video"
          : null;
      if (kind) {
        // Images show a downscaled preview; Download and video use the original.
        items.push({
          id: a.id,
          kind,
          src: kind === "image" ? preview(a.id) : orig(a.id),
          download: orig(a.id),
          alt: a.filename,
          filename: a.filename,
        });
      }
    }
  }
  if (items.length === 0) return;
  const found = items.findIndex((i) => i.id === attachmentId);
  lightbox.set({ items, index: found < 0 ? 0 : found });
  pushHistory();
}

/// Open the viewer on an arbitrary gallery (e.g. saved items, an avatar) — not
/// tied to the message transcript. Same swipe/arrow navigation.
export function openGallery(items: LightboxItem[], index: number): void {
  if (items.length === 0) return;
  lightbox.set({ items, index: Math.max(0, Math.min(index, items.length - 1)) });
  pushHistory();
}

export function closeLightbox(): void {
  lightbox.set(null);
  // Closed via UI (Esc / ✕ / backdrop) → pop our own history entry to stay in
  // sync (fires popstate, but the store is already null so it's a no-op).
  if (historyPushed) {
    historyPushed = false;
    if (typeof history !== "undefined") history.back();
  }
}

/// Step the gallery; wraps around at the ends.
export function step(delta: number): void {
  lightbox.update((s) => {
    if (!s || s.items.length < 2) return s;
    const n = s.items.length;
    return { ...s, index: (s.index + delta + n) % n };
  });
}
