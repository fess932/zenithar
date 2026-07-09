// Sticker/emoji/gif packs — grouped сохранёнки. Thin client over /api/packs.
// A pack groups SavedItems under a name + kind (which sub-list it shows under).
// Shared by an unguessable slug: /?addpack=<slug> lets anyone copy the whole pack.
import { writable } from "svelte/store";
import type { SavedItem } from "./saved";

// Lottie animations (imported .tgs) are served with this content type so the UI
// renders them with the animation player instead of an <img>.
export const LOTTIE_CT = "application/lottie+json";

export type PackKind = "sticker" | "gif" | "saved";

export interface Pack {
  id: string;
  owner_id: string;
  name: string;
  kind: PackKind;
  public: boolean;
  cover_item_id: string | null;
  share_slug: string;
  created_at: number;
  items: SavedItem[];
}

// Which shared pack (by slug) the add-pack dialog should show; null = closed.
// Set from a share link (?addpack=) or by tapping a sticker's pack.
export const addPackSlug = writable<string | null>(null);
export function openAddPack(slug: string): void {
  addPackSlug.set(slug);
}

// Blobs are served `immutable` (cached a year). A pack item can change bytes in
// place (WebM→WebP convert reuses the same id), so pass a `v` cache-buster that
// changes with the content (its byte size) — else the browser keeps serving the
// stale WebM from cache and an <img> can't decode it (stuck skeleton).
/// A pack item's own blob (owner view). Shared previews use `sharedItemUrl`.
export const packItemUrl = (id: string, v?: number | string): string =>
  `/api/saved/${id}/file${v != null ? `?v=${v}` : ""}`;
/// A shared pack's item blob — readable by anyone who has the slug.
export const sharedItemUrl = (slug: string, id: string, v?: number | string): string =>
  `/api/packs/${slug}/items/${id}/file${v != null ? `?v=${v}` : ""}`;

/// The link to share a pack: opening it prompts the recipient to add the pack.
export const packShareUrl = (slug: string): string =>
  `${location.origin}/?addpack=${slug}`;

export function isLottie(ct: string): boolean {
  return ct === LOTTIE_CT;
}

export function isVideoSticker(ct: string): boolean {
  return ct.startsWith("video/");
}

/// True if the pack still holds old WebM (video) stickers — offer to convert them
/// to the lighter animated-WebP format.
export function packHasWebm(pack: Pack): boolean {
  return pack.items.some((it) => it.content_type === "video/webm");
}

export async function listPacks(): Promise<Pack[]> {
  try {
    const r = await fetch("/api/packs");
    return r.ok ? ((await r.json()) as Pack[]) : [];
  } catch {
    return [];
  }
}

/// Another user's PUBLIC packs (for their profile).
export async function listPacksOf(principalId: string): Promise<Pack[]> {
  try {
    const r = await fetch(`/api/packs/of/${principalId}`);
    return r.ok ? ((await r.json()) as Pack[]) : [];
  } catch {
    return [];
  }
}

/// Show a pack on the owner's profile (public) or hide it (private).
export async function setPackPublic(id: string, isPublic: boolean): Promise<boolean> {
  return patchPack(id, { public: isPublic });
}

export async function createPack(name: string, kind: PackKind): Promise<Pack | null> {
  try {
    const r = await fetch("/api/packs", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name, kind }),
    });
    return r.ok ? ((await r.json()) as Pack) : null;
  } catch {
    return null;
  }
}

/// Import a .wastickers/.zip archive or a single .tgs as a new pack. With no
/// `kind`, the server infers it from the archive contents (tgs/webm/webp →
/// sticker, gif → gif, plain photos → saved).
export async function importPack(
  file: File,
  kind?: PackKind,
  name?: string,
): Promise<Pack | null> {
  const fd = new FormData();
  fd.append("file", file, file.name);
  if (kind) fd.append("kind", kind);
  if (name) fd.append("name", name);
  try {
    const r = await fetch("/api/packs/import", { method: "POST", body: fd });
    return r.ok ? ((await r.json()) as Pack) : null;
  } catch {
    return null;
  }
}

/// Add one file (image or .tgs) to an existing pack.
export async function addPackItem(packId: string, file: File): Promise<SavedItem | null> {
  const fd = new FormData();
  fd.append("file", file, file.name);
  try {
    const r = await fetch(`/api/packs/${packId}/items`, { method: "POST", body: fd });
    return r.ok ? ((await r.json()) as SavedItem) : null;
  } catch {
    return null;
  }
}

export async function renamePack(id: string, name: string): Promise<boolean> {
  return patchPack(id, { name });
}

/// Move a pack to a different sub-list (sticker / gif / saved).
export async function setPackKind(id: string, kind: PackKind): Promise<boolean> {
  return patchPack(id, { kind });
}

/// Convert a pack's WebM (video) stickers to animated WebP in place. Returns the
/// refreshed pack (or null on failure).
export async function convertPack(id: string): Promise<Pack | null> {
  try {
    const r = await fetch(`/api/packs/${id}/convert`, { method: "POST" });
    return r.ok ? ((await r.json()) as Pack) : null;
  } catch {
    return null;
  }
}

async function patchPack(
  id: string,
  body: { name?: string; kind?: PackKind; public?: boolean },
): Promise<boolean> {
  try {
    return (
      await fetch(`/api/packs/${id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      })
    ).ok;
  } catch {
    return false;
  }
}

export async function deletePack(id: string): Promise<boolean> {
  try {
    return (await fetch(`/api/packs/${id}`, { method: "DELETE" })).ok;
  } catch {
    return false;
  }
}

export async function deletePackItem(packId: string, itemId: string): Promise<boolean> {
  try {
    return (await fetch(`/api/packs/${packId}/items/${itemId}`, { method: "DELETE" })).ok;
  } catch {
    return false;
  }
}

/// Preview a shared pack by slug (no auth to the owner needed).
export async function previewPack(slug: string): Promise<Pack | null> {
  try {
    const r = await fetch(`/api/packs/${slug}/preview`);
    return r.ok ? ((await r.json()) as Pack) : null;
  } catch {
    return null;
  }
}

/// Copy a shared pack into the caller's own collection.
export async function addPackBySlug(slug: string): Promise<Pack | null> {
  try {
    const r = await fetch(`/api/packs/${slug}/add`, { method: "POST" });
    return r.ok ? ((await r.json()) as Pack) : null;
  } catch {
    return null;
  }
}
