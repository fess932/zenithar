// Telegram-style press-and-hold preview for picker items. A tile opens this on a
// long-press (big centred preview); a quick tap sends instead. The overlay
// (StickerPreview.svelte) is mounted once and driven by this store.
import { writable } from "svelte/store";

export type PreviewKind = "lottie" | "webm" | "img" | "video";

export interface StickerPreviewState {
  src: string;
  kind: PreviewKind;
  alt: string;
  send: (() => void) | null; // null in read-only views (no send affordance)
}

export const stickerPreview = writable<StickerPreviewState | null>(null);

export function openStickerPreview(s: StickerPreviewState): void {
  stickerPreview.set(s);
}
export function closeStickerPreview(): void {
  stickerPreview.set(null);
}
