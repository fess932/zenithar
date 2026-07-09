#!/usr/bin/env python3
"""Tiny WebM -> animated WebP transcoder sidecar.

The main (distroless) backend stays ffmpeg-free; it POSTs a WebM sticker here and
gets an animated WebP back. Video stickers thus render as a plain <img> in the
app (no per-sticker <video> decoder → no jank) instead of WebM.

Endpoints:
  GET  /health                       -> "ok"
  POST /webm2webp[?size=&fps=&q=]    -> body is WebM bytes, returns WebP bytes

Stdlib only (no pip deps). Binds loopback by default — it's an internal sidecar,
reached over host networking at 127.0.0.1:PORT, never exposed publicly.
"""
import os
import subprocess
import sys
import tempfile
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

HOST = os.environ.get("TRANSCODER_BIND", "127.0.0.1")
PORT = int(os.environ.get("TRANSCODER_PORT", "3001"))
MAX_BYTES = int(os.environ.get("TRANSCODER_MAX_BYTES", str(40 * 1024 * 1024)))


def log(*args):
    """Timestamped line to stdout, flushed so it shows up live in `docker logs`."""
    ts = time.strftime("%Y-%m-%d %H:%M:%S")
    print(f"[transcoder {ts}]", *args, flush=True)


def kb(n):
    return f"{n / 1024:.0f}KB"


def clamp(v, lo, hi, default):
    try:
        return max(lo, min(hi, int(v)))
    except (TypeError, ValueError):
        return default


def transcode(data: bytes, size: int, fps: int, q: int):
    """Run ffmpeg (decode webm -> scale/fps -> animated webp).

    Returns (webp_bytes, None) on success or (None, error_message) on failure.
    """
    with tempfile.TemporaryDirectory() as d:
        inp = os.path.join(d, "in.webm")
        out = os.path.join(d, "out.webp")
        with open(inp, "wb") as f:
            f.write(data)
        vf = (
            f"fps={fps},"
            f"scale={size}:{size}:force_original_aspect_ratio=decrease:flags=lanczos"
        )
        r = subprocess.run(
            [
                "ffmpeg", "-y", "-loglevel", "error", "-i", inp,
                "-vf", vf,
                "-c:v", "libwebp",
                "-q:v", str(q),
                "-compression_level", "6",
                "-loop", "0",
                "-an",
                out,
            ],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        if r.returncode != 0 or not os.path.exists(out):
            err = r.stderr.decode("utf-8", "replace").strip() or f"rc={r.returncode}"
            return None, err
        with open(out, "rb") as f:
            return f.read(), None


class Handler(BaseHTTPRequestHandler):
    def _send(self, code, body=b"", ctype="text/plain"):
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def do_GET(self):
        if urlparse(self.path).path == "/health":
            self._send(200, b"ok")
        else:
            self._send(404, b"not found")

    def do_POST(self):
        u = urlparse(self.path)
        if u.path != "/webm2webp":
            self._send(404, b"not found")
            return
        n = int(self.headers.get("Content-Length", "0") or "0")
        if n <= 0 or n > MAX_BYTES:
            log(f"✗ rejected: bad size ({n} bytes, max {MAX_BYTES})")
            self._send(413, b"bad size")
            return
        data = self.rfile.read(n)
        qs = parse_qs(u.query)
        size = clamp(qs.get("size", [None])[0], 64, 512, 256)
        fps = clamp(qs.get("fps", [None])[0], 5, 30, 24)
        q = clamp(qs.get("q", [None])[0], 1, 100, 65)
        params = f"size={size} fps={fps} q={q}"
        t0 = time.monotonic()
        out, err = transcode(data, size, fps, q)
        dt = time.monotonic() - t0
        if out is None:
            # Flatten ffmpeg's multi-line stderr into a single line, tail only.
            oneline = " ".join(err.split())[-300:]
            log(f"✗ webm2webp {kb(len(data))} {params} failed in {dt:.1f}s: {oneline}")
            self._send(500, b"transcode failed")
            return
        ratio = len(out) * 100 // max(1, len(data))
        log(f"✓ webm2webp {kb(len(data))}→{kb(len(out))} ({ratio}%) {params} in {dt:.1f}s")
        self._send(200, out, "image/webp")

    def log_message(self, *args):  # silence the default access log; we log our own
        pass


def check_ffmpeg():
    """Warn loudly at startup if this ffmpeg can't encode webp (the whole point)."""
    try:
        enc = subprocess.run(
            ["ffmpeg", "-hide_banner", "-encoders"],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
        ).stdout.decode("utf-8", "replace")
        if "libwebp" in enc:
            log("ffmpeg OK — libwebp encoder present")
        else:
            log("WARNING: ffmpeg has NO libwebp encoder — transcodes will fail!")
    except FileNotFoundError:
        log("WARNING: ffmpeg not found on PATH — transcodes will fail!")


if __name__ == "__main__":
    log(f"starting on {HOST}:{PORT} (max upload {MAX_BYTES // 1024 // 1024}MB)")
    check_ffmpeg()
    try:
        ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
    except KeyboardInterrupt:
        log("shutting down")
        sys.exit(0)
