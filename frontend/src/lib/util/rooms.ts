// Pure helpers for room labels/previews (chat list + header). Framework-free and
// dependency-free: takes a structural `RoomLike` (which the app's RoomSummary
// satisfies), so this never imports from the store layer. Unit-tested in
// rooms.test.ts.

export interface RoomLike {
  kind: string;
  title?: string | null;
  last_body?: string | null;
  last_author?: string | null;
}

/// Display name for a room: the common room's localized label, a `@handle` for a
/// DM peer, or the client's title.
export function roomLabel(r: RoomLike, commonLabel: string): string {
  if (r.kind === "common") return commonLabel;
  if (r.kind === "direct") return `@${r.title ?? "?"}`; // @handle = a person
  return r.title ?? "—";
}

/// Last-message preview for the chat list. Group-like rooms (common/client) prefix
/// the sender; a 1:1 direct room doesn't. Empty body = attachment-only → a clip.
export function roomPreview(r: RoomLike): string {
  const text = (r.last_body ?? "").trim() || "📎";
  if (r.kind === "direct") return text;
  return r.last_author ? `${r.last_author}: ${text}` : text;
}
