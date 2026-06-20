# Zenithar

Self-hosted corporate messenger with voice calls. Rust backend + TypeScript (bun) frontend.
See [requirements.md](requirements.md) and [docs/design-notes.md](docs/design-notes.md).

## Quickstart

Everything runs through the **Makefile** (`make` to list targets):

```sh
make dev        # debug server on http://127.0.0.1:3000 (reads frontend/dist from disk)
make fe-dev     # in another terminal: rebuild the frontend on change

make build      # full release build — frontend embedded into one self-contained binary
make run        # build frontend, then run the release server

make test       # backend tests + frontend typecheck
make e2e        # end-to-end auth/chat suite (spins up an ephemeral server)
make lint       # clippy + rustfmt check + typecheck
make fmt        # format
```

## Layout

- `backend/` — Rust (axum). WS chat at `/ws`, health at `/api/health`. SQLite via sqlx
  with a batching writer (≤50ms / ≤16 messages per transaction). DB lives in the
  git-ignored `backend/data/`. The built frontend is embedded via `rust-embed`.
- `frontend/` — TypeScript client built with bun. Single team room, realtime over WS.

## Auth (passwordless, link-based)

Everyone signs in by a personal **link-token** — no passwords. A token is
exchanged for an httpOnly cookie on first visit (`/i/<token>`). Identities
(`principals`) are either `user` (employee) or `client` (anonymous); admins
create them from the UI (the "Links" panel).

First run bootstraps an admin and surfaces its one-time login link: logged, and
saved to a git-ignored `.env` as `ZENITHAR_ADMIN_LINK` (in the server's working
dir, i.e. `backend/.env` for `make dev`/`make run`). Open `http://<host>/i/<token>`
once to sign in, then create the rest from the UI.
See [docs/plans/phase-1-auth.md](docs/plans/phase-1-auth.md).

## Status

- **Phase 0** — realtime chat in the common room with batched persistence.
- **Phase 1** — passwordless link auth: sessions, unified principals, link
  management UI, identity-stamped messages.

See the roadmap in [requirements.md](requirements.md).

## Integration API

A REST API under `/api/v1` lets integrations (CRM, bots) read and post to chats.
Authenticate with a permanent **Bearer token** (`zk_…`) created in the admin UI
(the "Integrations" section). An integration is a `bot` principal — a first-class
message author. Full reference: [docs/plans/phase-6-api.md](docs/plans/phase-6-api.md).

```sh
# create a client + login link, seeding the order as the first message
curl -X POST https://host/api/v1/clients \
  -H "Authorization: Bearer zk_…" -H "Content-Type: application/json" \
  -d '{"name":"Acme","order":"Order #123: laptop repair"}'
# → {"client_id":"…","room_id":"…","url":"/i/…"}

# post into a client's room by client_id
curl -X POST https://host/api/v1/clients/<client_id>/messages \
  -H "Authorization: Bearer zk_…" -H "Content-Type: application/json" \
  -d '{"body":"On it!"}'
```

Other routes: `GET /api/v1/me`, `GET /api/v1/rooms`,
`GET|POST /api/v1/rooms/{id}/messages` (history is paginated via `?limit&before`),
`POST /api/v1/uploads` (multipart, then pass the returned id in `attachment_ids`).

## Deploy (self-host)

One self-contained container; state in a `./data` volume. Full guide —
docker-compose, **host networking for calls**, Caddy + TLS, backups, env — in
[docs/deploy.md](docs/deploy.md).

```sh
mkdir -p data && docker compose up -d   # then open the admin link from the logs
```

## Config (env)

- `ZENITHAR_BIND` — listen address (default `127.0.0.1:3000`)
- `ZENITHAR_DB` — SQLite path (default `data/zenithar.db`, relative to `backend/`)
- `ZENITHAR_ATTACHMENTS` — uploads dir (default `<data>/attachments`)
- `ZENITHAR_RECORDINGS` — call recordings dir (default `<data>/recordings`)
- `ZENITHAR_STUN` — comma-separated STUN URLs for WebRTC ICE (empty = LAN/localhost)
- `ZENITHAR_PUBLIC_IP` — public IP(s) to advertise for calls when behind NAT/DMZ (NAT 1:1); see [docs/deploy.md](docs/deploy.md)
- `ZENITHAR_UDP_PORTS` — single media UDP port for calls (muxed, bound `0.0.0.0`), e.g. `51000` (forward just this port); empty = ephemeral
- `ZENITHAR_SECURE_COOKIES` — set `1`/`true` to mark auth cookies `Secure` (behind TLS)
- `RUST_LOG` — log filter (default `info`)
