// WebSocket chat state as Svelte stores: the transcript, the connection status,
// the rooms the caller can open, and the active room. Author comes from the
// authenticated identity (server-side); sends only carry body + a client id.
import { get, writable } from "svelte/store";
import { t } from "./i18n";

export interface Attachment {
  id: string;
  filename: string;
  content_type: string;
  size: number;
  width: number | null;
  height: number | null;
  has_thumb: boolean;
}

export interface ReplyPreview {
  id: string;
  author_name: string;
  body: string;
  has_attachment: boolean;
}

export interface ChatMessage {
  id: string;
  room_id: string;
  author_id: string;
  author_name: string;
  body: string;
  reply_to: ReplyPreview | null;
  client_msg_id: string | null;
  created_at: number; // unix millis
  attachments: Attachment[];
}

export interface RoomSummary {
  id: string;
  kind: "common" | "client";
  title: string | null; // client name; null for the common room
  created_at: number;
}

export type Status = "connecting" | "live" | "down";

// Server → client frames. Chat frames are handled here; `call-*` signaling
// frames are forwarded to a handler registered by the call layer.
type Frame =
  | { type: "history"; room_id: string; messages: ChatMessage[] }
  | { type: "message"; message: ChatMessage }
  | { type: string; [k: string]: unknown };

/// A unique id that works outside secure contexts too. `crypto.randomUUID` is
/// only defined on HTTPS/localhost, so over a plain-HTTP LAN IP it throws — this
/// falls back to a random-enough id there.
export function uuid(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  return `${Date.now().toString(16)}-${Math.random().toString(16).slice(2)}-${Math.random()
    .toString(16)
    .slice(2)}`;
}

// The call layer registers here to receive `call-*` signaling frames.
let signalHandler: ((f: Frame) => void) | null = null;
export function onSignal(handler: (f: Frame) => void): void {
  signalHandler = handler;
}

/// Send a raw frame over the shared socket. Returns false if it isn't open.
export function sendFrame(frame: unknown): boolean {
  if (ws?.readyState !== WebSocket.OPEN) return false;
  ws.send(JSON.stringify(frame));
  return true;
}

export const messages = writable<ChatMessage[]>([]);
export const status = writable<Status>("connecting");
export const rooms = writable<RoomSummary[]>([]);
export const activeRoom = writable<string | null>(null);

/// The message the composer is currently replying to (Telegram-style), or null.
export const replyingTo = writable<ChatMessage | null>(null);

/// Briefly highlighted message id — set when jumping to a quoted original.
export const highlightId = writable<string | null>(null);
let highlightTimer: ReturnType<typeof setTimeout> | null = null;

/// Flash a message (used after scrolling to a reply's original).
export function flashMessage(id: string): void {
  highlightId.set(id);
  if (highlightTimer) clearTimeout(highlightTimer);
  highlightTimer = setTimeout(() => highlightId.set(null), 1600);
}

/// Transient, user-visible error banner (also logged to the console).
export const notice = writable<string | null>(null);
let noticeTimer: ReturnType<typeof setTimeout> | null = null;

function flash(msg: string): void {
  console.error("[zenithar]", msg);
  notice.set(msg);
  if (noticeTimer) clearTimeout(noticeTimer);
  noticeTimer = setTimeout(() => notice.set(null), 6000);
}

export function dismissNotice(): void {
  notice.set(null);
  if (noticeTimer) clearTimeout(noticeTimer);
}

/// Surface a transient error toast from a component.
export function notify(msg: string): void {
  flash(msg);
}

let ws: WebSocket | null = null;
let backoff = 500;

export function connect(): void {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  ws = new WebSocket(`${proto}://${location.host}/ws`);
  status.set("connecting");

  ws.onopen = () => {
    backoff = 500;
    status.set("live");
    // Restore the room we were viewing (server otherwise picks the default).
    const want = get(activeRoom);
    if (want) ws?.send(JSON.stringify({ type: "join", room_id: want }));
  };
  ws.onmessage = (ev) => {
    let f: Frame;
    try {
      f = JSON.parse(ev.data) as Frame;
    } catch {
      return; // ignore non-JSON frames
    }
    if (f.type === "history") {
      activeRoom.set((f as { room_id: string }).room_id);
      messages.set((f as { messages: ChatMessage[] }).messages);
    } else if (f.type === "message") {
      const msg = (f as { message: ChatMessage }).message;
      if (msg.room_id !== get(activeRoom)) return; // not the open room
      messages.update((all) => [...all, msg]);
    } else if (f.type.startsWith("call-")) {
      signalHandler?.(f);
    }
  };
  ws.onclose = () => {
    status.set("down");
    setTimeout(connect, backoff);
    backoff = Math.min(backoff * 2, 8000);
  };
  ws.onerror = () => ws?.close();
}

/// Switch the open room (employees only; clients have a single room).
export function joinRoom(room_id: string): void {
  if (get(activeRoom) === room_id) return;
  activeRoom.set(room_id);
  replyingTo.set(null); // a reply target doesn't carry across rooms
  messages.set([]); // history frame will repopulate
  if (ws?.readyState === WebSocket.OPEN) {
    ws.send(JSON.stringify({ type: "join", room_id }));
  }
}

export function send(
  body: string,
  attachmentIds: string[] = [],
  replyToId: string | null = null,
): boolean {
  if (ws?.readyState !== WebSocket.OPEN) {
    flash(get(t)("errSend"));
    return false;
  }
  ws.send(
    JSON.stringify({
      type: "msg",
      body,
      client_msg_id: uuid(),
      attachment_ids: attachmentIds,
      reply_to: replyToId,
    }),
  );
  return true;
}

/// Upload a file/image/voice clip to the active room; returns its metadata.
export async function uploadFile(file: File): Promise<Attachment | null> {
  const room = get(activeRoom);
  if (!room) {
    flash(get(t)("errUpload"));
    return null;
  }
  const fd = new FormData();
  fd.append("room_id", room);
  fd.append("file", file, file.name);
  try {
    const r = await fetch("/api/upload", { method: "POST", body: fd });
    if (!r.ok) {
      flash(`${get(t)("errUpload")} (${r.status})`);
      return null;
    }
    return (await r.json()) as Attachment;
  } catch {
    flash(get(t)("errUpload"));
    return null;
  }
}

export async function loadRooms(): Promise<void> {
  try {
    const r = await fetch("/api/rooms");
    rooms.set(r.ok ? ((await r.json()) as RoomSummary[]) : []);
  } catch {
    rooms.set([]);
  }
}
