// WebSocket chat state as Svelte stores: the message transcript and the
// connection status. Author comes from the authenticated identity (server-side);
// sends only carry body + a client id for idempotency.
import { writable } from "svelte/store";

export interface ChatMessage {
  id: string;
  room_id: string;
  author_id: string;
  author_name: string;
  body: string;
  client_msg_id: string | null;
  created_at: number; // unix millis
}

export type Status = "connecting" | "live" | "down";

export const messages = writable<ChatMessage[]>([]);
export const status = writable<Status>("connecting");

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

export function send(body: string): boolean {
  if (ws?.readyState !== WebSocket.OPEN) return false;
  ws.send(JSON.stringify({ body, client_msg_id: crypto.randomUUID() }));
  return true;
}
