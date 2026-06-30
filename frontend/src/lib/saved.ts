// "Сохранёнки" — a user's private saved-image collection. Thin client over the
// /api/saved endpoints. Items hold their own blob server-side, so they outlive
// the messages they were saved from.
import { get } from "svelte/store";
import { activeRoom, send, type Attachment } from "./chat";

export interface SavedItem {
  id: string;
  filename: string;
  content_type: string;
  size: number;
  width: number | null;
  height: number | null;
  has_thumb: boolean;
  public: boolean;
  created_at: number;
}

export const savedUrl = (id: string): string => `/api/saved/${id}/file`;
export const savedThumb = (id: string): string => `/api/saved/${id}/thumb`;

export async function listSaved(): Promise<SavedItem[]> {
  try {
    const r = await fetch("/api/saved");
    return r.ok ? ((await r.json()) as SavedItem[]) : [];
  } catch {
    return [];
  }
}

export async function listSavedOf(principalId: string): Promise<SavedItem[]> {
  try {
    const r = await fetch(`/api/saved/of/${principalId}`);
    return r.ok ? ((await r.json()) as SavedItem[]) : [];
  } catch {
    return [];
  }
}

/// Save an image from a message into your collection (server copies the blob).
export async function saveFromMessage(attachmentId: string): Promise<boolean> {
  try {
    return (await fetch(`/api/saved/from/${attachmentId}`, { method: "POST" })).ok;
  } catch {
    return false;
  }
}

export async function uploadSaved(file: File): Promise<SavedItem | null> {
  const fd = new FormData();
  fd.append("file", file, file.name);
  try {
    const r = await fetch("/api/saved/upload", { method: "POST", body: fd });
    return r.ok ? ((await r.json()) as SavedItem) : null;
  } catch {
    return null;
  }
}

export async function setSavedPublic(id: string, isPublic: boolean): Promise<boolean> {
  try {
    return (
      await fetch(`/api/saved/${id}`, {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ public: isPublic }),
      })
    ).ok;
  } catch {
    return false;
  }
}

export async function deleteSaved(id: string): Promise<boolean> {
  try {
    return (await fetch(`/api/saved/${id}`, { method: "DELETE" })).ok;
  } catch {
    return false;
  }
}

/// Send a saved item into the open room: the server copies it to a room
/// attachment, then we send that attachment as a normal message.
export async function sendSaved(id: string): Promise<boolean> {
  const room = get(activeRoom);
  if (!room) return false;
  try {
    const r = await fetch(`/api/saved/${id}/attach`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ room_id: room }),
    });
    if (!r.ok) return false;
    const att = (await r.json()) as Attachment;
    return send("", [att.id], null);
  } catch {
    return false;
  }
}
