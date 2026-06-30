// Built-in sticker set. Stickers are sent by id (a reference, not bytes) and both
// sides render the bundled asset from /assets/stickers/. Adding a sticker = drop
// its file in src/assets/stickers/ and add an entry here. Format is inferred from
// the file extension so the player can pick the renderer (Lottie/WebM/WebP/GIF).
//
// Starter set: Noto Animated Emoji (Google, Apache-2.0), as Lottie JSON.

export type StickerFormat = "lottie" | "webm" | "webp" | "gif";

export interface StickerDef {
  id: string;
  emoji: string; // a quick label / fallback glyph for the picker
  file: string; // filename under /assets/stickers/
}

export const STICKERS: StickerDef[] = [
  { id: "heart", emoji: "❤️", file: "heart.json" },
  { id: "thumbsup", emoji: "👍", file: "thumbsup.json" },
  { id: "joy", emoji: "😂", file: "joy.json" },
  { id: "fire", emoji: "🔥", file: "fire.json" },
  { id: "party", emoji: "🎉", file: "party.json" },
  { id: "rocket", emoji: "🚀", file: "rocket.json" },
];

const BY_ID = new Map(STICKERS.map((s) => [s.id, s]));

export function sticker(id: string): StickerDef | undefined {
  return BY_ID.get(id);
}

export function stickerUrl(s: StickerDef): string {
  return `/assets/stickers/${s.file}`;
}

export function formatOf(file: string): StickerFormat {
  if (file.endsWith(".webm")) return "webm";
  if (file.endsWith(".webp")) return "webp";
  if (file.endsWith(".gif")) return "gif";
  return "lottie"; // .json / .lottie
}
