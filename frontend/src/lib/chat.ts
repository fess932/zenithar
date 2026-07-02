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

// One emoji's reactions on a message: the principal ids who reacted. count =
// by.length; it's "mine" when my id is in `by`.
export interface Reactor {
  id: string;
  avatar: string | null;
}
export interface Reaction {
  emoji: string;
  by: Reactor[];
}

export interface ChatMessage {
  id: string;
  room_id: string;
  author_id: string;
  author_name: string;
  author_avatar?: string | null; // emoji / "photo:<millis>" / null → default
  body: string;
  reply_to: ReplyPreview | null;
  client_msg_id: string | null;
  created_at: number; // unix millis
  edited_at: number | null; // set when the author edits the body
  attachments: Attachment[];
  reactions: Reaction[];
  sticker: string | null; // sticker id when this is a sticker message (body empty)
}

export interface RoomSummary {
  id: string;
  kind: "common" | "client" | "direct";
  title: string | null; // client/DM-peer name; null for the common room
  client_id: string | null; // client (or DM peer) id for the online dot; null for common
  created_at: number;
  last_at: number | null; // time of the last message (chat-list preview); null if none
  last_body: string | null; // last message body; "" for attachment-only messages
  last_author: string | null; // last message sender's name
}

/// One online principal (presence snapshot entry).
export interface PresenceEntry {
  id: string;
  kind: string;
}

/// A heads-up that an anonymous client wrote in their room (employees only).
export interface ClientNotice {
  room_id: string;
  from_name: string;
  preview: string;
  created_at: number;
}

/// A light nudge that someone reacted to YOUR message (not a message itself).
export interface ReactionNotice {
  room_id: string;
  message_id: string;
  emoji: string;
  from_name: string;
}

export type Status = "connecting" | "live" | "down";

// Server → client frames. Chat frames are handled here; `call-*` signaling
// frames are forwarded to a handler registered by the call layer.
type Frame =
  | { type: "history"; room_id: string; messages: ChatMessage[] }
  | { type: "message"; message: ChatMessage }
  | { type: "client-notice"; notice: ClientNotice }
  | { type: "presence-snapshot"; online: PresenceEntry[] }
  | { type: "presence"; id: string; kind: string; online: boolean }
  | { type: "unread-counts"; counts: Record<string, number> }
  | { type: "unread"; room_id: string }
  | { type: "message-edited"; id: string; room_id: string; body: string; edited_at: number }
  | { type: "message-deleted"; id: string; room_id: string }
  | { type: "message-reaction"; id: string; room_id: string; reactions: Reaction[] }
  | { type: "reaction-notice"; room_id: string; message_id: string; emoji: string; from_name: string }
  | { type: "read"; room_id: string; principal_id: string; at: number }
  | { type: "read-state"; room_id: string; others_read_at: number }
  | { type: "rooms-changed" }
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

// The notification layer registers here to react (sound/toast) to client notices.
let clientNoticeHandler: ((n: ClientNotice) => void) | null = null;
export function onClientNotice(handler: (n: ClientNotice) => void): void {
  clientNoticeHandler = handler;
}

// The notification layer registers here for the quiet "someone reacted" nudge.
let reactionNoticeHandler: ((r: ReactionNotice) => void) | null = null;
export function onReactionNotice(handler: (r: ReactionNotice) => void): void {
  reactionNoticeHandler = handler;
}

// Fires for every message that lands in the room currently open (including your
// own echo). The notification layer uses it to chime on an incoming reply —
// notably so an anonymous client hears an employee's answer, not just the other
// way round.
let incomingHandler: ((m: ChatMessage) => void) | null = null;
export function onIncoming(handler: (m: ChatMessage) => void): void {
  incomingHandler = handler;
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

// Older-history pagination (lazy load on scroll-up). `hasMoreHistory` flips false
// once a short page comes back; `loadingOlder` guards against concurrent loads.
const HISTORY_PAGE = 50;
export const hasMoreHistory = writable(true);
let loadingOlder = false;

// Cap the in-memory transcript so an active room can't grow without bound (memory
// + DOM nodes). Only trimmed while the view is pinned to the bottom — scrolling up
// to read history must never yank messages out from under the reader. Trimmed-off
// older messages reload via `loadOlder` on scroll-up.
const MAX_LIVE_MESSAGES = 300;
let viewPinned = true;
/// Chat.svelte reports whether the transcript is scrolled to the bottom, gating
/// the transcript-cap trim above.
export function setViewPinned(v: boolean): void {
  viewPinned = v;
}

// Remember the open room across reloads: persist whenever it changes, and rejoin
// it on (re)connect if it still exists (the server falls back to the default room
// otherwise). Lets a refresh keep you where you were.
const ROOM_KEY = "zenithar.room";
activeRoom.subscribe((r) => {
  try {
    if (r) localStorage.setItem(ROOM_KEY, r);
  } catch {
    /* private mode — in-memory only */
  }
});
function rememberedRoom(): string | null {
  try {
    return localStorage.getItem(ROOM_KEY);
  } catch {
    return null;
  }
}

/// Currently-online principals: id → kind. Reset whenever the socket reconnects
/// (a fresh snapshot follows). Lets the UI show online dots / counts.
export const online = writable<Record<string, string>>({});

/// The message the composer is currently replying to (Telegram-style), or null.
export const replyingTo = writable<ChatMessage | null>(null);
/// The message currently being edited (composer switches to edit mode), or null.
export const editing = writable<ChatMessage | null>(null);

/// Edit a message's body (author only — server enforces).
export function editMessage(id: string, body: string): void {
  sendFrame({ type: "edit", id, body });
}

/// Delete a message (author or admin — server enforces).
export function deleteMessage(id: string): void {
  sendFrame({ type: "delete", id });
}

/// Toggle an emoji reaction on a message (add if absent, remove if present). The
/// server broadcasts the new set back as a `message-reaction` frame.
export function toggleReaction(id: string, emoji: string): void {
  sendFrame({ type: "react", id, emoji });
}

/// Unread anonymous-client messages per room (cleared when the room is opened).
/// Counts even for muted rooms — muting only silences sound/popups, see notify.ts.
export const unread = writable<Record<string, number>>({});

/// Read receipts: per room, the newest timestamp OTHERS have read to. Your own
/// message is "read" (✓✓) once this ≥ its `created_at`, else just delivered (✓).
export const readAt = writable<Record<string, number>>({});

/// Tell the server we've read `room` up to `at` (its latest message's time).
export function sendRead(room: string, at: number): void {
  sendFrame({ type: "read", room_id: room, at });
}

function clearUnread(room_id: string): void {
  unread.update((u) => {
    if (!(room_id in u)) return u;
    const next = { ...u };
    delete next[room_id];
    return next;
  });
}

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
// Single reconnect timer so a manual resync + the auto-backoff can't both fire
// connect() and spawn parallel sockets (which would duplicate every message).
let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

export function connect(): void {
  // Single-flight: never open a second socket while one is live or opening.
  if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) return;
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  const proto = location.protocol === "https:" ? "wss" : "ws";
  ws = new WebSocket(`${proto}://${location.host}/ws`);
  status.set("connecting");

  ws.onopen = () => {
    backoff = 500;
    status.set("live");
    // Restore the room we were viewing (in memory after a reconnect, or from
    // localStorage after a full reload). The server denies + ignores it if the
    // room no longer exists, leaving us on the default it already sent.
    const want = get(activeRoom) ?? rememberedRoom();
    if (want) ws?.send(JSON.stringify({ type: "join", room_id: want }));
    // Refresh the rooms list on every (re)connect: its previews/unread aren't
    // pushed live, so after a drop the list would otherwise stay stale.
    void loadRooms();
    // Flush anything composed while offline (idempotent via client_msg_id).
    flushPending();
  };
  ws.onmessage = (ev) => {
    let f: Frame;
    try {
      f = JSON.parse(ev.data) as Frame;
    } catch {
      return; // ignore non-JSON frames
    }
    if (f.type === "history") {
      const room = (f as { room_id: string }).room_id;
      activeRoom.set(room);
      clearUnread(room); // viewing it now → no longer unread
      const msgs = (f as { messages: ChatMessage[] }).messages;
      messages.set(msgs);
      hasMoreHistory.set(msgs.length >= HISTORY_PAGE); // a full page → maybe more
      if (msgs.length) sendRead(room, msgs[msgs.length - 1].created_at); // read receipt
    } else if (f.type === "message") {
      const msg = (f as { message: ChatMessage }).message;
      if (msg.room_id !== get(activeRoom)) return; // not the open room
      const next = [...get(messages), msg];
      if (viewPinned && next.length > MAX_LIVE_MESSAGES) {
        messages.set(next.slice(-MAX_LIVE_MESSAGES));
        hasMoreHistory.set(true); // we dropped older ones → they're loadable again
      } else {
        messages.set(next);
      }
      sendRead(msg.room_id, msg.created_at); // we're viewing it → mark read
      incomingHandler?.(msg);
    } else if (f.type === "read") {
      const e = f as { room_id: string; at: number }; // always someone else (server excludes us)
      readAt.update((r) => ({ ...r, [e.room_id]: Math.max(r[e.room_id] ?? 0, e.at) }));
    } else if (f.type === "read-state") {
      const e = f as { room_id: string; others_read_at: number };
      readAt.update((r) => ({ ...r, [e.room_id]: Math.max(r[e.room_id] ?? 0, e.others_read_at) }));
    } else if (f.type === "message-edited") {
      const e = f as { id: string; body: string; edited_at: number };
      messages.update((all) =>
        all.map((m) => (m.id === e.id ? { ...m, body: e.body, edited_at: e.edited_at } : m)),
      );
    } else if (f.type === "message-deleted") {
      const id = (f as { id: string }).id;
      messages.update((all) => all.filter((m) => m.id !== id));
    } else if (f.type === "message-reaction") {
      const e = f as { id: string; reactions: Reaction[] };
      messages.update((all) =>
        all.map((m) => (m.id === e.id ? { ...m, reactions: e.reactions ?? [] } : m)),
      );
    } else if (f.type === "reaction-notice") {
      reactionNoticeHandler?.(
        f as { room_id: string; message_id: string; emoji: string; from_name: string },
      );
    } else if (f.type === "rooms-changed") {
      void loadRooms(); // e.g. someone opened a DM with us — refresh the list
    } else if (f.type === "client-notice") {
      const n = (f as { notice: ClientNotice }).notice;
      // Sound/toast only (the unread count comes from the "unread" frame below,
      // which covers every room — not just anonymous-client ones).
      if (n.room_id !== get(activeRoom)) clientNoticeHandler?.(n);
    } else if (f.type === "unread-counts") {
      // Authoritative per-room counts from the server (survives reload).
      unread.set((f as { counts: Record<string, number> }).counts ?? {});
    } else if (f.type === "unread") {
      const room = (f as { room_id: string }).room_id;
      if (room !== get(activeRoom)) {
        unread.update((u) => ({ ...u, [room]: (u[room] ?? 0) + 1 }));
      }
    } else if (f.type === "presence-snapshot") {
      const list = (f as { online: PresenceEntry[] }).online;
      online.set(Object.fromEntries(list.map((p) => [p.id, p.kind])));
    } else if (f.type === "presence") {
      const p = f as { id: string; kind: string; online: boolean };
      online.update((cur) => {
        const next = { ...cur };
        if (p.online) next[p.id] = p.kind;
        else delete next[p.id];
        return next;
      });
    } else if (f.type.startsWith("call-")) {
      signalHandler?.(f);
    }
  };
  ws.onclose = () => {
    status.set("down");
    online.set({}); // stale until the next snapshot
    ws = null; // let the single-flight guard permit the next connect()
    if (reconnectTimer) clearTimeout(reconnectTimer);
    reconnectTimer = setTimeout(connect, backoff);
    backoff = Math.min(backoff * 2, 8000);
  };
  ws.onerror = () => ws?.close();
}

// Messages composed while the socket is down wait here and flush on reconnect.
// `client_msg_id` makes a resend idempotent, so a flush can't duplicate.
interface OutMsg {
  body: string;
  attachment_ids: string[];
  reply_to: string | null;
  client_msg_id: string;
  sticker?: string; // set only for sticker messages
}
let pending: OutMsg[] = [];
const MAX_PENDING = 50;

function transmit(m: OutMsg): boolean {
  if (ws?.readyState !== WebSocket.OPEN) return false;
  ws.send(JSON.stringify({ type: "msg", ...m }));
  return true;
}

function flushPending(): void {
  if (pending.length === 0) return;
  const queue = pending;
  pending = [];
  for (const m of queue) {
    if (!transmit(m)) pending.push(m); // socket closed again mid-flush
  }
}

/// Switch the open room (employees only; clients have a single room).
export function joinRoom(room_id: string): void {
  if (get(activeRoom) === room_id) return;
  activeRoom.set(room_id);
  replyingTo.set(null); // a reply target doesn't carry across rooms
  editing.set(null); // an in-progress edit doesn't carry across rooms
  clearUnread(room_id);
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
  const m: OutMsg = {
    body,
    attachment_ids: attachmentIds,
    reply_to: replyToId,
    client_msg_id: uuid(),
  };
  // If the socket is down, queue and let onopen flush it (instead of failing).
  if (!transmit(m)) {
    if (pending.length >= MAX_PENDING) {
      flash(get(t)("errSend"));
      return false;
    }
    pending.push(m);
  }
  return true;
}

/// Send a sticker (by bundled id) as its own message — a reference, not bytes.
export function sendSticker(id: string): boolean {
  const m: OutMsg = {
    body: "",
    attachment_ids: [],
    reply_to: null,
    client_msg_id: uuid(),
    sticker: id,
  };
  if (!transmit(m)) {
    if (pending.length >= MAX_PENDING) {
      flash(get(t)("errSend"));
      return false;
    }
    pending.push(m);
  }
  return true;
}

/// Per-upload size ceilings — mirror the backend so the UI rejects oversized files
/// up front instead of waiting for a 413. Videos get 200 MB, everything else 40 MB.
export const MAX_UPLOAD_BYTES = 40 * 1024 * 1024;
export const MAX_VIDEO_BYTES = 200 * 1024 * 1024;
export const uploadLimit = (file: File): number =>
  file.type.startsWith("video/") ? MAX_VIDEO_BYTES : MAX_UPLOAD_BYTES;

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

/// Manual recovery (pull-to-refresh): refetch the rooms list and bounce the
/// socket so it reconnects and re-pulls the open room's history — recovers from a
/// half-dead WS where messages silently stopped arriving.
export function resync(): void {
  void loadRooms();
  if (ws && ws.readyState === WebSocket.OPEN) ws.close();
  else connect();
}

export async function loadRooms(): Promise<void> {
  try {
    const r = await fetch("/api/rooms");
    rooms.set(r.ok ? ((await r.json()) as RoomSummary[]) : []);
  } catch {
    rooms.set([]);
  }
}

/// Fetch a page of messages older than what's loaded and prepend them. Returns
/// how many were added (0 = nothing older / not applicable). Drives lazy
/// scroll-up loading in the chat.
export async function loadOlder(): Promise<number> {
  if (loadingOlder || !get(hasMoreHistory)) return 0;
  const room = get(activeRoom);
  const oldest = get(messages)[0]?.id;
  if (!room || !oldest) return 0;
  loadingOlder = true;
  try {
    const r = await fetch(`/api/rooms/${room}/messages?before=${oldest}&limit=${HISTORY_PAGE}`);
    if (!r.ok) return 0;
    const older = (await r.json()) as ChatMessage[]; // oldest-first
    if (older.length < HISTORY_PAGE) hasMoreHistory.set(false);
    // Room may have changed while the request was in flight.
    if (older.length === 0 || get(activeRoom) !== room) return 0;
    messages.update((all) => [...older, ...all]);
    return older.length;
  } catch {
    return 0;
  } finally {
    loadingOlder = false;
  }
}
