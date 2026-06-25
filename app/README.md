# Zenithar desktop app (Tauri v2)

A thin native window (macOS + Windows for now) onto a self-hosted Zenithar server.
The **web frontend runs unchanged** inside it: the window loads the server's own
URL, so the web app stays same-origin and never depends on Tauri.

## Server URL

Set it in [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) →
`app.windows[0].url` (currently `https://chat.re-star.ru`). One build = one
server; rebuild to point elsewhere. (A configurable URL + token provisioning is a
planned follow-up.)

## Build locally (macOS)

From the repo root:

```sh
make app-mac     # builds the web bundle, generates icons, bundles the .app/.dmg
make app-dev     # run against the live server in a dev window
```

Output lands in `app/src-tauri/target/release/bundle/` (`.dmg`, `.app`).

Windows builds run in CI (you can't cross-compile Tauri from macOS — WebView2 +
MSVC + NSIS are Windows-only).

## CI

The `desktop` job in [`.github/workflows/docker.yml`](../.github/workflows/docker.yml)
builds macOS + Windows on their native runners (after the backend image, so it
never slows the backend build) and uploads the installers as artifacts.

## Icons

The icon set is generated from [`app-icon.png`](app-icon.png) (1024×1024) by
`tauri icon` during the build, so only that source PNG is committed. Replace it to
rebrand.

## Status / caveats

- Unsigned builds → macOS Gatekeeper / Windows SmartScreen will warn. Code signing
  is added later via certs in CI secrets.
- This scaffold follows Tauri v2 conventions but was authored without a local
  Tauri toolchain; the first `make app-mac` / CI run is the real validation.
