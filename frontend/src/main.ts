// Zenithar team room — Phase 0 client.
// Connects to the WS relay, renders the message transcript, sends messages with
// a client-generated id (idempotency) and reconnects with backoff.

interface ChatMessage {
  id: string;
  room_id: string;
  author: string;
  body: string;
  client_msg_id: string | null;
  created_at: number; // unix millis
}

const log = document.getElementById("log") as HTMLElement;
const empty = document.getElementById("empty") as HTMLElement | null;
const form = document.getElementById("composer") as HTMLFormElement;
const nameInput = document.getElementById("name") as HTMLInputElement;
const bodyInput = document.getElementById("body") as HTMLInputElement;
const beacon = document.getElementById("beacon") as HTMLElement;
const beaconLabel = document.getElementById("beacon-label") as HTMLElement;

// Our own sends, so we can mark our lines without waiting for the echo.
const mine = new Set<string>();

const NAME_KEY = "zenithar.name";
nameInput.value = localStorage.getItem(NAME_KEY) ?? "";
nameInput.addEventListener("change", () =>
  localStorage.setItem(NAME_KEY, nameInput.value.trim()),
);

function setBeacon(state: "connecting" | "live" | "down", label: string): void {
  beacon.dataset.state = state;
  beaconLabel.textContent = label;
}

function fmtTime(ms: number): string {
  const d = new Date(ms);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function render(m: ChatMessage): void {
  empty?.remove();
  const isMine = m.client_msg_id !== null && mine.has(m.client_msg_id);

  const line = document.createElement("div");
  line.className = isMine ? "line mine arrived" : "line arrived";

  const time = document.createElement("span");
  time.className = "time";
  time.textContent = fmtTime(m.created_at);

  const who = document.createElement("span");
  who.className = "who";
  who.textContent = isMine ? "you" : m.author;

  const body = document.createElement("span");
  body.className = "body";
  body.textContent = m.body;

  line.append(time, who, body);

  const atBottom =
    log.scrollHeight - log.scrollTop - log.clientHeight < 80;
  log.appendChild(line);
  if (atBottom || isMine) log.scrollTop = log.scrollHeight;
}

// ---- websocket with reconnect/backoff ------------------------------------
let ws: WebSocket | null = null;
let backoff = 500;

function connect(): void {
  const proto = location.protocol === "https:" ? "wss" : "ws";
  ws = new WebSocket(`${proto}://${location.host}/ws`);
  setBeacon("connecting", "connecting");

  ws.onopen = () => {
    backoff = 500;
    setBeacon("live", "live");
  };
  ws.onmessage = (ev) => {
    try {
      render(JSON.parse(ev.data) as ChatMessage);
    } catch {
      /* ignore non-JSON frames */
    }
  };
  ws.onclose = () => {
    setBeacon("down", "reconnecting");
    setTimeout(connect, backoff);
    backoff = Math.min(backoff * 2, 8000);
  };
  ws.onerror = () => ws?.close();
}

form.addEventListener("submit", (e) => {
  e.preventDefault();
  const body = bodyInput.value.trim();
  if (!body || ws?.readyState !== WebSocket.OPEN) return;

  const clientMsgId = crypto.randomUUID();
  mine.add(clientMsgId);
  ws.send(
    JSON.stringify({
      body,
      author: nameInput.value.trim() || "anon",
      client_msg_id: clientMsgId,
    }),
  );
  bodyInput.value = "";
  bodyInput.focus();
});

connect();
