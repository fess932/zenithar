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

## Android — local development setup (macOS)

One-time prerequisites: **Java 17**, the **Android SDK + NDK**, the **Rust Android
targets**, and a few env vars. (Examples use the `fish` shell; for zsh/bash use
`export NAME=value`.)

**1. Java 17** — Gradle won't run on a newer JDK:

```fish
brew install --cask temurin@17
set -gx JAVA_HOME (/usr/libexec/java_home -v 17)
```

**2. Android SDK + NDK** — easiest via **Android Studio** (also gives you an
emulator). First launch downloads the SDK to `~/Library/Android/sdk`; then in
*SDK Manager*:

- **SDK Platforms** → *Android 16 (API 36)*
- **SDK Tools** → *Build-Tools*, *Command-line Tools*, *Platform-Tools*, and
  **NDK (Side by side)**

Or pure CLI:

```fish
brew install --cask android-commandlinetools
sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;27.2.12479018"
sdkmanager --licenses
```

**3. Env vars** — add to `~/.config/fish/config.fish`:

```fish
set -gx ANDROID_HOME "$HOME/Library/Android/sdk"
set -gx NDK_HOME "$ANDROID_HOME/ndk/27.2.12479018"   # match your installed version: `ls $ANDROID_HOME/ndk`
set -gx JAVA_HOME (/usr/libexec/java_home -v 17)
```

**4. Rust Android targets:**

```fish
rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android
```

**5. Build / run:**

```fish
make app-android                   # tauri android init + build → debug .apk
cd app && bun run tauri android dev   # live run on a device/emulator (hot reload)
```

`make app-android` is self-contained on macOS: it auto-resolves `JAVA_HOME` (17),
`ANDROID_HOME`, and `NDK_HOME`, and **installs the NDK** if `sdkmanager`
(cmdline-tools) is present — so steps 1–4's env vars are only needed for
`tauri android dev`. If you have neither the NDK nor cmdline-tools, the quickest
path is `brew install --cask android-commandlinetools`, then `make app-android`
installs the NDK for you. Override the SDK path with `make app-android ANDROID_SDK=/path`.

`tauri android dev` needs a **connected phone** (USB debugging on) **or a running
emulator** (Android Studio → Device Manager). The debug `.apk` lands in
`app/src-tauri/gen/android/app/build/outputs/apk/…` and sideloads as-is.

The CI `android` job does the same on a Linux runner (Java 17 pinned) and uploads
the APK. A Play-Store `.aab` needs a release keystore (later).

### SDK levels

- **minSdk 24** (Android 7.0) — set in `tauri.conf.json` → `bundle.android.minSdkVersion`;
  Tauri's floor, ~99% device coverage. Survives `tauri android init`.
- **compileSdk / targetSdk 36** (Android 16) — these live in the *generated*
  `gen/android/app/build.gradle.kts`, so `make app-android` (and CI) patch them to
  36 after `init`. (Google Play requires target 36 for new apps/updates from
  2026-08-31; irrelevant for sideloaded APKs, but we stay current.) Needs the
  `platforms;android-36` SDK installed.

> **Deep links on Android** use an intent-filter generated into the manifest under
> `gen/android` — verify it carries the `zenithar` scheme after the first
> `tauri android init` (the desktop deep-link path is already wired).

> **Common gotchas:** `NDK_HOME` not set / wrong version; default JDK is newer than
> 17 (set `JAVA_HOME`); licenses not accepted (`sdkmanager --licenses`).

## CI

In [`.github/workflows/docker.yml`](../.github/workflows/docker.yml), the `desktop`
job builds macOS + Windows on their native runners and the `android` job builds
the APK on Linux. Both run **after** the backend image (so they never slow the
backend build) and upload installers/APKs as artifacts.

## Icons

The icon set is generated from [`app-icon.png`](app-icon.png) (1024×1024) by
`tauri icon` during the build, so only that source PNG is committed. Replace it to
rebrand.

## Status / caveats

- Unsigned builds → macOS Gatekeeper / Windows SmartScreen will warn. Code signing
  is added later via certs in CI secrets.
- This scaffold follows Tauri v2 conventions but was authored without a local
  Tauri toolchain; the first `make app-mac` / CI run is the real validation.
