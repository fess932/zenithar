# WebM -> animated WebP transcoder sidecar. Debian's ffmpeg is built with
# libwebp, so no custom build needed. Stdlib-only Python server (no pip deps).
FROM python:3.12-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY server.py /app/server.py

# Loopback by default — reached over host networking at 127.0.0.1:PORT, never
# exposed publicly. Override TRANSCODER_BIND/PORT via compose if needed.
# PYTHONUNBUFFERED so logs appear live in `docker logs` (no stdout buffering).
ENV TRANSCODER_BIND=127.0.0.1 \
    TRANSCODER_PORT=3001 \
    PYTHONUNBUFFERED=1

WORKDIR /app
EXPOSE 3001
CMD ["python3", "server.py"]
