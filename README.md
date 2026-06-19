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
make lint       # clippy + rustfmt check + typecheck
make fmt        # format
```

## Layout

- `backend/` — Rust (axum). WS chat at `/ws`, health at `/api/health`. SQLite via sqlx
  with a batching writer (≤50ms / ≤16 messages per transaction). DB lives in the
  git-ignored `backend/data/`. The built frontend is embedded via `rust-embed`.
- `frontend/` — TypeScript client built with bun. Single team room, realtime over WS.

## Status

Phase 0 (skeleton) is in place: realtime chat in the common room with batched
persistence. See the roadmap in [requirements.md](requirements.md).

## Config (env)

- `ZENITHAR_BIND` — listen address (default `127.0.0.1:3000`)
- `ZENITHAR_DB` — SQLite path (default `data/zenithar.db`, relative to `backend/`)
