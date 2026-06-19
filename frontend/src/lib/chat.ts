// WebSocket chat state as Svelte stores: the message transcript and the
// connection status. Sends carry a client id (idempotency); reconnect backs off.
import { writable } from "svelte/store";

export interface ChatMessage {
  id: string;
  room_id: string;
  author: string;
  body: string;
  client_msg_id: string | null;
  created_at: number; // unix millis
}

export type Status = "connecting" | "live" | "down";

export const messages = writable<ChatMessage[]>([]);
export const status = writable<Status>("connecting");

// Ids we sent ourselves, so we can mark our own lines without waiting on the echo.
const mine = new Set<string>();

export function isMine(m: ChatMessage): boolean {
  return m.client_msg_id !== null && mine.has(m.client_msg_id);
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
  };
  ws.onmessage = (ev) => {
    try {
      const m = JSON.parse(ev.data) as ChatMessage;
      messages.update((all) => [...all, m]);
    } catch {
      /* ignore non-JSON frames */
    }
  };
  ws.onclose = () => {
    status.set("down");
    setTimeout(connect, backoff);
    backoff = Math.min(backoff * 2, 8000);
  };
  ws.onerror = () => ws?.close();
}

export function send(body: string, author: string): boolean {
  if (ws?.readyState !== WebSocket.OPEN) return false;
  const clientMsgId = crypto.randomUUID();
  mine.add(clientMsgId);
  ws.send(JSON.stringify({ body, author, client_msg_id: clientMsgId }));
  return true;
}
