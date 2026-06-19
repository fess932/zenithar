# Self-hosting Zenithar

Zenithar ships as **one self-contained container**: the frontend is built and
embedded into the Rust binary, which serves the SPA, the API, the WebSocket
(chat + call signaling), and terminates WebRTC media. State (SQLite DB,
attachments, call recordings, bootstrap admin link) lives in a single `/data`
volume.

The image is built on GitHub Actions and pushed to GHCR:
`ghcr.io/fess932/zenithar:latest`.

---

## 1. Quick start (docker compose)

```sh
mkdir -p data            # persistent state lives here (bind-mounted to /data)
docker compose up -d     # pulls/builds the image and starts the server
docker compose logs -f   # watch the logs
```

On the **first run** Zenithar bootstraps an admin and prints a one-time login
link, also saved to `data/.env` as `ZENITHAR_ADMIN_LINK`:

```
first run — admin login link (open once, also saved to .env): http://0.0.0.0:3000/i/<token>
```

Open `http://<your-host>/i/<token>` once to sign in, then create the rest of the
users/clients from the **Links** panel in the UI.

> `docker` may be aliased to `podman`. Both work; with podman, build with
> `--format docker` if you want the image-level metadata preserved (the
> healthcheck itself lives in `docker-compose.yml`, not the image).

---

## 2. Voice calls need host networking

WebRTC **media** is UDP on ephemeral ports. Bridged Docker networking does **not**
forward those, so calls won't carry audio under the default `ports:` mapping —
only the signaling (over `:3000`) would work.

On Linux, run the container with **host networking**. In `docker-compose.yml`
remove the `ports:` block and uncomment:

```yaml
    network_mode: host
```

The server then binds the host's interfaces directly and ICE can negotiate UDP.
Pinning a fixed UDP port range instead of host networking is a possible future
refinement.

### Server behind NAT / DMZ → set `ZENITHAR_PUBLIC_IP`

If the box only has a **private** address on its interface (e.g. `10.51.0.10`)
and is reached from outside via a **public IP that a router/DMZ forwards** to it,
ICE will otherwise only gather that private candidate — and a remote browser
can't connect (`ICE failed` on the client, `could not get server reflexive
address … deadline has elapsed` on the server). This is the common self-host
case.

Fix: advertise the public IP (NAT 1:1). The server adds a server-reflexive
candidate with the public IP **and keeps its private host candidate**, so a
caller on the same LAN still connects directly (no NAT hairpin) and an external
caller uses the public one. No STUN/TURN needed.

```yaml
    environment:
      ZENITHAR_PUBLIC_IP: "203.0.113.7"   # your external IP (comma-separate for several)
      ZENITHAR_STUN: ""                    # drop public STUN; it's not needed and may be blocked
```

The router/DMZ must forward UDP (and `:3000` TCP for signaling) to the server.
With full port forwarding (DMZ) nothing else is required. **Do not rely on public
STUN like Google's** — it's often blocked (e.g. from RU) and just stalls ICE
gathering; `ZENITHAR_PUBLIC_IP` makes the server reachable deterministically.

---

## 3. Reverse proxy + TLS (Caddy)

Run Zenithar behind a TLS-terminating reverse proxy. Caddy gets you automatic
Let's Encrypt certificates with a two-line config. With the app on host
networking at `:3000`:

```caddyfile
chat.example.com {
    reverse_proxy 127.0.0.1:3000
}
```

Caddy proxies HTTP **and** WebSocket (`/ws`) transparently. Once you're on HTTPS,
set `ZENITHAR_SECURE_COOKIES=1` so the session cookie is marked `Secure`.

### Gotcha: a broken IPv6 route stalls cert issuance

If Caddy hangs getting a certificate (`http2: timeout awaiting response headers`
talking to `acme-v02.api.letsencrypt.org`) while plain `curl` to Let's Encrypt
works, suspect a **non-routable global IPv6** address on the host (e.g. a
`fc..::/…` ULA with `scope global` on a bridge like `lxcbr0`). Go's dialer then
tries IPv6 first and stalls. Fix by disabling IPv6 (persist it):

```sh
echo -e 'net.ipv6.conf.all.disable_ipv6 = 1\nnet.ipv6.conf.default.disable_ipv6 = 1' \
  | sudo tee /etc/sysctl.d/99-disable-ipv6.conf
sudo sysctl --system
```

Or, more surgically, stop advertising that global-scope ULA on the bridge.

---

## 4. Configuration (environment)

| Variable | Default | Purpose |
|---|---|---|
| `ZENITHAR_BIND` | `127.0.0.1:3000` (image: `0.0.0.0:3000`) | Listen address. |
| `ZENITHAR_DB` | `data/zenithar.db` (image: `/data/zenithar.db`) | SQLite path; its parent dir is the data dir. |
| `ZENITHAR_ATTACHMENTS` | `<data>/attachments` | Uploaded files on disk. |
| `ZENITHAR_RECORDINGS` | `<data>/recordings` | Server-side call recordings (`<call_id>.<participant_id>.ogg`). |
| `ZENITHAR_STUN` | — | Comma-separated STUN URLs for ICE. Empty = host candidates (LAN/localhost). |
| `ZENITHAR_PUBLIC_IP` | — | Public IP(s) to advertise as host candidates (NAT 1:1). Set on a server behind NAT/DMZ so remote browsers can reach the media path. |
| `ZENITHAR_SECURE_COOKIES` | `0` | `1`/`true` to mark the auth cookie `Secure` (behind TLS). |
| `RUST_LOG` | `info` | Log filter (`tracing` env-filter syntax). |

`ZENITHAR_ADMIN_LINK` is **written** by the app to `<data>/.env` on first run
(the bootstrap admin's link); you don't set it.

### Built-in abuse limits (Phase 7)

Sensible defaults, no config needed: per-socket message rate limit (~1/s,
burst 10), per-principal upload limit (burst 30), and per-IP `/i/:token` login
throttle (burst 20). Behind a proxy these read the client IP from
`X-Forwarded-For` / `X-Real-IP` (Caddy sets these by default).

---

## 5. Backups

Everything to back up is under `./data`:

- `zenithar.db` (+ `-wal`, `-shm`) — messages, principals, tokens, sessions, calls.
- `attachments/` — uploaded files and image thumbnails.
- `recordings/` — call recordings.
- `.env` — the bootstrap admin link (recoverable by reissuing from the UI).

SQLite is in WAL mode. For a consistent copy while running, prefer:

```sh
docker compose exec zenithar sh -c 'true'   # (no shell in distroless)
# Simplest robust approach: stop, copy, start.
docker compose stop
cp -a data data-backup-$(date +%F)
docker compose start
```

(SQLite tolerates copying the live files in WAL mode, but a brief stop guarantees
a clean snapshot.)

---

## 6. Updating

```sh
docker compose pull      # fetch the new :latest (or pin a :sha-/tag)
docker compose up -d      # recreate with the new image
```

The schema is created idempotently on start (`CREATE TABLE IF NOT EXISTS …`).
Pre-1.0 there are **no incremental migrations**: a breaking schema change means
recreating the DB. Pin a specific tag (e.g. `:sha-abc1234`) in production if you
want reproducible deploys instead of tracking `:latest`.
```
