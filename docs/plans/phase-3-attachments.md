# Phase 3 — Attachments

Image and file attachments on chat messages.

## Decisions

- **Storage: local disk behind a `Storage` trait.** Files live in a git-ignored
  dir next to the SQLite DB; metadata in the DB. The trait (sync, `Arc<dyn Storage>`,
  called via `spawn_blocking`) lets an S3/MinIO backend drop in later without
  rewriting callers.
- **Server-side thumbnails for images** (`image` crate). On upload we detect images,
  record dimensions, and generate a small JPEG thumbnail. Non-images are stored as-is.

## Data model (edit the single `0001_init.sql`, recreate via `make db-reset`)

```sql
CREATE TABLE attachments (
  id           TEXT PRIMARY KEY,          -- ULID; also the disk key
  room_id      TEXT NOT NULL REFERENCES rooms(id),
  uploader_id  TEXT NOT NULL REFERENCES principals(id),
  filename     TEXT NOT NULL,             -- original name (display only)
  content_type TEXT NOT NULL,             -- sniffed/validated mime
  size         INTEGER NOT NULL,          -- bytes
  width        INTEGER, height INTEGER,   -- images only
  has_thumb    INTEGER NOT NULL DEFAULT 0,
  created_at   INTEGER NOT NULL
);
```

`messages` gains `attachment_id TEXT REFERENCES attachments(id)` (nullable).

Disk layout (under `data/attachments/`): `<id>` = original, `<id>.thumb` = JPEG thumb.

## Flow

1. `POST /api/upload` (multipart `room_id` + `file`, Identity-gated). Validate room
   access (employee → any room; client → own), cap size (25 MB). Detect image →
   dims + thumbnail; else store as generic. Insert `attachments`, return its JSON.
2. Client sends a WS `msg` with `attachment_id` (+ optional caption body). Server
   verifies the attachment is in the active room, embeds its metadata in the
   broadcast/persisted `ChatMessage`, persists `attachment_id`.
3. `GET /api/attachments/:id` (original) and `/api/attachments/:id/thumb` — Identity-
   gated + room-access checked; content-type + long cache headers.

## Backend changes

- **`Cargo.toml`** — `axum` feature `multipart`; `image` (png/jpeg/gif/webp).
- **`storage.rs`** (new) — `Storage` trait + `DiskStorage { root }`; key sanitization.
- **`migrations/0001_init.sql`** — `attachments` table + `messages.attachment_id`.
- **`models.rs`** — `Attachment`; `ChatMessage` gains `attachment: Option<Attachment>`
  (no longer a direct `FromRow` — built from a LEFT JOIN row).
- **`db.rs`** — attachment insert/lookup; `messages` queries LEFT JOIN attachments;
  `room_of_client` for access checks.
- **`uploads.rs`** (new) — upload handler (validate, sniff, thumbnail) + serve
  handlers (original/thumb) with room-access gating.
- **`ws.rs`** — `Inbound::Msg.attachment_id`; embed attachment on send.
- **`writer.rs`** — INSERT includes `attachment_id`.
- **`state.rs` / `main.rs`** — `storage: Arc<dyn Storage>`; wire routes; ensure dir.
- **`auth`/shared** — `can_access_room(principal, room_id)` helper.

## Frontend changes

- **`chat.ts`** — `ChatMessage.attachment`; `uploadFile(file)`; `send(body, attachmentId?)`.
- **`Composer.svelte`** — paperclip → file picker; pending-attachment chip with
  remove; allow send with empty text when an attachment is attached; mobile-first.
- **`Message.svelte`** — render image thumbnail (links to original) or a file chip
  (name + size + download).
- **i18n** — upload/attach/remove/download keys (RU default).

## Tests

- e2e: upload a small PNG in `common`, send it, assert the thumbnail `<img>` renders;
  upload a non-image and assert a download chip with the filename.
- Reject oversize / verify access gating (API-level).

## Added in this phase (beyond the original brief)

- **Voice messages** — recorded in the browser with `MediaRecorder`
  (echo-cancel / noise-suppress / AGC via `getUserMedia` constraints), uploaded as
  a normal audio attachment, played inline with `<audio controls>`. No extra backend.
- **Emoji** — a built-in offline picker (`lib/emoji.ts`, native Unicode emoji).
  Deliberately not a CDN-backed library (jsDelivr/unpkg data sources are unreliable
  from RU and not offline) — everything is local, so it works self-hosted anywhere.

## Out of scope (later)

S3 backend, virus scanning, drag-and-drop/paste upload, multiple attachments per
message, **stickers** (a bundled local open image set, e.g. OpenMoji), waveform UI
for voice.
