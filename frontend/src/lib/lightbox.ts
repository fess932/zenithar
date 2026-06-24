// An in-app media viewer (lightbox) with gallery navigation. Opening any image
// or video attachment gathers every image AND video in the current transcript
// into an ordered gallery so you can page through them (arrows / swipe) without
// leaving the app — videos play inline with native controls.
import { get, writable } from "svelte/store";
import { messages } from "./chat";

export interface LightboxItem {
  id: string;
  kind: "image" | "video";
  src: string; // full-resolution URL
  alt: string;
  filename: string;
}

interface LightboxState {
  items: LightboxItem[];
  index: number;
}

export const lightbox = writable<LightboxState | null>(null);

const orig = (id: string) => `/api/attachments/${id}`;

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
        items.push({ id: a.id, kind, src: orig(a.id), alt: a.filename, filename: a.filename });
      }
    }
  }
  if (items.length === 0) return;
  const found = items.findIndex((i) => i.id === attachmentId);
  lightbox.set({ items, index: found < 0 ? 0 : found });
}

export function closeLightbox(): void {
  lightbox.set(null);
}

/// Step the gallery; wraps around at the ends.
export function step(delta: number): void {
  lightbox.update((s) => {
    if (!s || s.items.length < 2) return s;
    const n = s.items.length;
    return { ...s, index: (s.index + delta + n) % n };
  });
}
