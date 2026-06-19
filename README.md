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

## Config (env)

- `ZENITHAR_BIND` — listen address (default `127.0.0.1:3000`)
- `ZENITHAR_DB` — SQLite path (default `data/zenithar.db`, relative to `backend/`)
- `ZENITHAR_SECURE_COOKIES` — set `1`/`true` to mark auth cookies `Secure` (behind TLS)
