# Phase 2 — Rooms

Multi-room chat on top of the Phase 1 passwordless auth.

## Product model (decided)

- **Per-client private room + internal `common`.**
  - `common` — internal team channel, **employees only**. Clients never see it.
  - Each **client** gets a dedicated room: that client + all employees.
  - A **client** sees only their own room.
  - An **employee** sees `common` + every client room.
- **No assignment / handoff.** Every employee can see and answer in every client
  room. No owner, no queue (can be added later).

## What already exists

The Phase 1 schema is already room-aware — no migration changes needed:

- `rooms(id, kind 'common'|'client', client_id → principals, created_at)`, seeds `common`.
- `messages(room_id → rooms, ...)`, indexed by `(room_id, id)`.

The gap is the runtime: `ws.rs` hardcodes the `common` room and uses a single
global broadcast (everyone gets everything).

## Backend changes

1. **`state.rs`** — broadcast carries the message (so subscribers can filter by
   room): `broadcast::Sender<ChatMessage>` instead of `Sender<String>`.
2. **`db.rs`**
   - `ensure_client_room(write, client_id) -> room_id` (idempotent; ULID room).
   - `room_exists(reads, room_id) -> bool` (employee join validation).
   - `list_rooms_for_user(reads) -> Vec<RoomSummary>` (common first, then clients
     by `created_at`, joined to `principals` for the client name).
3. **`models.rs`** — `RoomSummary { id, kind, title: Option<String>, created_at }`.
   `title` = client display name; `None` for `common` (frontend localizes it).
4. **`ws.rs`** — rewrite as a single `select!` loop (so a `join` can reply with
   history on the same socket):
   - Access: `client` → only their room (auto-created on connect); `user` → any
     room, default `common`.
   - Tagged client→server frames: `{type:"join",room_id}`, `{type:"msg",body,client_msg_id}`.
   - Tagged server→client frames: `{type:"history",room_id,messages}`, `{type:"message",message}`.
   - Forward a broadcast only when `msg.room_id == active_room` (no cross-room leak,
     trivial client logic). `join` validates access, swaps `active_room`, replays history.
5. **`routes.rs`** — `GET /api/rooms` (Identity-gated): client → their one room
   (ensured); user → `list_rooms_for_user`. On `create_principal(kind=client)`
   also `ensure_client_room`.
6. **`main.rs`** — broadcast channel type; wire `/api/rooms`.

## Frontend changes

- **`chat.ts`** — `rooms` + `activeRoom` stores; `joinRoom(id)` (clears transcript,
  sends `join`); parse tagged frames (`history` replaces transcript + sets active
  room, `message` appends if it matches active room); `send` emits `{type:"msg"}`;
  `loadRooms()` fetches `/api/rooms`.
- **`Chat.svelte`** — employees get a **rooms drawer** (mobile-first slide-in;
  toggle shows the current room name). Clients have a single room → no switcher.
- **i18n** — `rooms`, `noRooms` keys (RU default).

## Tests

- Existing specs keep passing (admin defaults to `common`; client defaults to its room).
- New e2e: admin opens a client's room, posts; the client (separate context)
  receives it; and the message does **not** appear in `common`.

## Out of scope (later)

Unread badges / cross-room notifications, assignment/handoff, room archiving,
typing indicators.
