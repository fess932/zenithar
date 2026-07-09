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
import tempfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse, parse_qs

HOST = os.environ.get("TRANSCODER_BIND", "127.0.0.1")
PORT = int(os.environ.get("TRANSCODER_PORT", "3001"))
MAX_BYTES = int(os.environ.get("TRANSCODER_MAX_BYTES", str(40 * 1024 * 1024)))


def clamp(v, lo, hi, default):
    try:
        return max(lo, min(hi, int(v)))
    except (TypeError, ValueError):
        return default


def transcode(data: bytes, size: int, fps: int, q: int):
    """Run ffmpeg (decode webm -> scale/fps -> animated webp). Returns bytes or None."""
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
            stderr=subprocess.DEVNULL,
        )
        if r.returncode != 0 or not os.path.exists(out):
            return None
        with open(out, "rb") as f:
            return f.read()


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
            self._send(413, b"bad size")
            return
        data = self.rfile.read(n)
        qs = parse_qs(u.query)
        size = clamp(qs.get("size", [None])[0], 64, 512, 256)
        fps = clamp(qs.get("fps", [None])[0], 5, 30, 24)
        q = clamp(qs.get("q", [None])[0], 1, 100, 65)
        out = transcode(data, size, fps, q)
        if not out:
            self._send(500, b"transcode failed")
            return
        self._send(200, out, "image/webp")

    def log_message(self, *args):  # keep logs quiet
        pass


if __name__ == "__main__":
    ThreadingHTTPServer((HOST, PORT), Handler).serve_forever()
