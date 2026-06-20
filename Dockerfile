# Zenithar — single self-contained image.
#
# Build order mirrors `make build`: build the frontend bundle, embed it into the
# Rust binary at compile time (rust-embed `#[folder = "../frontend/dist"]`), then
# ship ONLY that one binary in a minimal runtime image. No Node/Bun, no source,
# no frontend assets at runtime — the binary serves everything.

# 1. Frontend: Svelte + Tailwind → frontend/dist (index.html, main.js, styles.css)
FROM oven/bun:1.3 AS frontend
WORKDIR /app/frontend
# Deps first for layer caching.
COPY frontend/package.json frontend/bun.lock ./
RUN bun install --frozen-lockfile
COPY frontend/ ./
RUN bun run build

# 2. Backend: compile the release binary with the frontend embedded.
FROM rust:1-slim-bookworm AS backend
# build-essential: bundled SQLite (sqlx) + ring (DTLS). autotools/pkg-config:
# audiopus_sys builds libopus from source (autogen.sh → configure → make) and
# statically links it, so the distroless runtime needs nothing extra.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential autoconf automake libtool pkg-config \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app/backend
# Warm the dependency cache: build deps against a dummy main, so a code-only
# change doesn't recompile the whole webrtc/sqlx tree.
COPY backend/Cargo.toml backend/Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release \
    && rm -rf src
# Real sources + the embedded frontend (path is relative to the crate root).
COPY backend/ ./
COPY --from=frontend /app/frontend/dist /app/frontend/dist
# `touch` is essential: COPY restores the context's (older) mtimes, so without
# it cargo sees the dummy-built artifact as newer and skips the real build,
# shipping the empty `fn main(){}` stub. Bump mtimes to force a recompile.
RUN touch src/*.rs && cargo build --release
# Seed an empty /data to hand to the (shell-less) distroless runtime below.
RUN mkdir -p /seed-data

# 3. Runtime: just the binary on distroless/cc — glibc + libgcc (matches the
# builder), no shell, no package manager. Runs as root so it can always write a
# bind-mounted ./data that the host created as root (no manual chown needed).
FROM gcr.io/distroless/cc-debian12 AS runtime
COPY --from=backend /app/backend/target/release/zenithar-backend /usr/local/bin/zenithar-backend
# /data holds the SQLite DB, attachments, future call recordings, and the
# bootstrap admin link (.env, written to the working dir on first run).
COPY --from=backend /seed-data /data
ENV ZENITHAR_BIND=0.0.0.0:3000 \
    ZENITHAR_DB=/data/zenithar.db \
    ZENITHAR_ATTACHMENTS=/data/attachments \
    RUST_LOG=info
WORKDIR /data
VOLUME ["/data"]
EXPOSE 3000
# Health probe lives in docker-compose.yml (the binary can probe itself via
# `zenithar-backend healthcheck`); no image-level HEALTHCHECK.
ENTRYPOINT ["zenithar-backend"]
