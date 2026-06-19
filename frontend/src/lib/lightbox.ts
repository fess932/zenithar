// An in-app image viewer (lightbox) with gallery navigation. Opening any image
// attachment gathers every image in the current transcript into an ordered
// gallery so you can page through them (arrows / swipe) without leaving the app.
import { get, writable } from "svelte/store";
import { messages } from "./chat";

export interface LightboxImage {
  id: string;
  src: string; // full-resolution URL
  alt: string;
  filename: string;
}

interface LightboxState {
  items: LightboxImage[];
  index: number;
}

export const lightbox = writable<LightboxState | null>(null);

const orig = (id: string) => `/api/attachments/${id}`;

/// Open the viewer at the given attachment, with all transcript images as the
/// gallery (so prev/next can page through them).
export function openLightbox(attachmentId: string): void {
  const items: LightboxImage[] = [];
  for (const m of get(messages)) {
    for (const a of m.attachments) {
      if (a.content_type.startsWith("image/")) {
        items.push({ id: a.id, src: orig(a.id), alt: a.filename, filename: a.filename });
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
