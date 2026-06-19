# Phase 4 — Voice calls (WebRTC, server in the media path)

Real-time **voice-only** calls. The server is a WebRTC peer in the middle
(SFU-lite): every browser negotiates **one PeerConnection with the server**, and
the server forwards RTP between participants in the same call. Designed so
**Phase 5 (server-side recording)** is a tap on streams the server already
terminates — not a second media path.

## Decisions

- **Engine: `webrtc-rs` (own SFU-lite).** Pure Rust, zero external services
  (no LiveKit/Redis/Egress), works self-hosted from anywhere incl. RU. The server
  terminates DTLS/SRTP, so it has the raw Opus packets — recording (Phase 5) is
  "write the packets you're already forwarding to an `.ogg`". Matches the project
  ethos ([[decision-sqlite-batched-writes]], no-CDN [[decision-attachments-and-emoji]]).
- **Topology: browser ⇄ server only.** Browsers never peer directly; ICE/SDP is
  exchanged with the server alone. This satisfies "media через сервер" and means
  no browser-to-browser candidate relaying.
- **Scope: 1:1 first.** A call lives in a chat room (client room → client↔employee;
  common → two employees). Group fan-out in the same room is a later step; the
  call/participant model is built to extend to N without a protocol change.
- **Codec: Opus** (`getUserMedia` with echoCancellation/noiseSuppression/AGC,
  reusing the constraints already proven for voice messages). No video, ever.
- **ICE: STUN only.** Server has a public IP and is always reachable, so no TURN.
  Configurable STUN list; empty list works on LAN/localhost (host candidates).
- **Signaling rides the existing `/ws`.** New tagged frames alongside chat; the
  author/identity is the authenticated principal, never the client frame.

## Signaling protocol (new `/ws` frames)

Call control + SDP/ICE are **addressed**, unlike chat which is room-broadcast.
A new `signal` broadcast channel carries frames stamped with a target principal
(or call id); each socket forwards only what is addressed to it.

Client → server (`Inbound`):
- `call-start { room_id }` — open (or join) the room's call; server replies with
  an SDP offer once the server PeerConnection is ready.
- `call-answer { call_id, sdp }` / `call-offer { call_id, sdp }` — SDP exchange.
- `call-ice { call_id, candidate }` — trickled ICE candidate.
- `call-leave { call_id }` — hang up / leave.

Server → client (`Outbound`):
- `call-ringing { call_id, room_id, from }` — someone started a call in your room.
- `call-offer { call_id, sdp }` / `call-answer { call_id, sdp }` — SDP from server.
- `call-ice { call_id, candidate }` — server's trickled candidate.
- `call-state { call_id, participants }` — join/leave updates for the UI.
- `call-ended { call_id }`.

## Call & media model (server)

- A **`Call`** is keyed by `room_id` (one active call per room for now), with a
  `call_id` (ULID), a map of `participant_id → ServerPeer`, and a recording sink.
- A **`ServerPeer`** wraps a `webrtc::RTCPeerConnection` plus the participant's
  inbound audio track. On a remote track arriving, its RTP is (a) written to every
  *other* participant's outbound track in the same call, and (b) handed to the
  recorder (Phase 5).
- **`CallRegistry`** in `AppState` (`Arc<Mutex<HashMap<room_id, Call>>>`) owns
  lifecycle: create on first `call-start`, drop when the last participant leaves
  (which also finalizes any recording).

## Data model (edit the single `0001_init.sql`, recreate via `make db-reset`)

Calls are logged for history/recording metadata (Phase 5 fills `recording_*`):

```sql
CREATE TABLE calls (
  id            TEXT PRIMARY KEY,           -- ULID = call_id
  room_id       TEXT NOT NULL REFERENCES rooms(id),
  started_by    TEXT NOT NULL REFERENCES principals(id),
  started_at    INTEGER NOT NULL,           -- unix millis
  ended_at      INTEGER,                    -- NULL while live
  recording_id  TEXT                        -- Phase 5: attachment/blob key
);
CREATE INDEX idx_calls_room ON calls(room_id);
```

A call may surface in the transcript as a system message ("Звонок · 2:14") once
ended — kept minimal in this phase.

## Backend changes

- **`Cargo.toml`** — `webrtc` crate (full stack: ICE/DTLS/SRTP/RTP/SDP).
- **`calls.rs`** (new) — `Call`, `ServerPeer`, `CallRegistry`; SDP/ICE handling,
  RTP forwarding between participants, a `Recorder` seam (no-op now; Phase 5 muxes
  Opus → `.ogg` via `Storage`).
- **`models.rs`** — `Inbound`/`Outbound` gain the `call-*` variants above; a
  `Signal { target: String, frame: Outbound }` wrapper for addressed fan-out.
- **`state.rs`** — `calls: CallRegistry`; `signal: broadcast::Sender<Signal>`;
  STUN config.
- **`ws.rs`** — subscribe to the `signal` channel; in the `select!` loop, handle
  `call-*` inbound (delegate to `CallRegistry`) and forward `signal` frames
  addressed to this principal. Chat path is untouched.
- **`db.rs`** — `insert_call`, `end_call` (+ Phase 5 recording fields).
- **config** — `ZENITHAR_STUN` (comma-separated), default empty.

## Frontend changes

- **`call.ts`** (new) — `RTCPeerConnection` to the server: getUserMedia(audio),
  add track, handle server offer/answer, trickle ICE over `/ws`; stores for call
  state (`idle | ringing | connecting | live | ended`), participants, mute, and a
  call timer. Reuses the WS in `chat.ts` (shared socket).
- **`Call.svelte`** (new) — in-room call bar: start/join, accept/decline on
  `ringing`, mute toggle, elapsed timer, hang up. Mobile-first per
  [[feedback-mobile-first-design]]; styled to match the beacon/voice player.
- **`Chat.svelte`** — a call button in the room bar; render `Call.svelte` when a
  call is active/ringing in the open room.
- **i18n** — call/start/join/accept/decline/mute/hangup/ringing/connecting keys
  (RU default + EN).

## Tests

- e2e is hard for real media in headless Chromium, so cover what's deterministic:
  - **Signaling handshake (API/WS):** two contexts in a room; A `call-start` →
    both see `ringing`/`offer`; B answers; assert `call-state` shows 2 participants;
    B leaves → `call-ended`. Chromium exposes `RTCPeerConnection`, so a real SDP
    offer/answer round-trip against the server is feasible with `--use-fake-...`
    media flags; gate behind a tag if flaky.
  - Access gating: a principal can only start/join a call in a room it can access.
- Keep the chat suite green (signaling must not disturb the chat broadcast path).

## Phase 5 seam (server-side recording) — designed in now

The `Recorder` trait is introduced here as a no-op. Because the server already
holds decrypted Opus RTP per participant, Phase 5 only adds: an Ogg/Opus writer
fed from the forward loop, a blob written via `Storage`, and `calls.recording_id`
set on `end_call`. No media-path change.

## Out of scope (later)

Group calls (N-way fan-out), video, screen share, TURN, simulcast/bandwidth
adaptation, call history UI beyond a system line, push/ring when app is closed,
the actual recording (Phase 5).
