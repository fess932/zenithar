# Zenithar — single entry point for all dev/build/test/lint tasks.
# Backend: Rust (cargo). Frontend: TypeScript (bun). Run `make` for the list.

BACKEND  := backend
FRONTEND := frontend

# Listen address (override: `make dev BIND=0.0.0.0:8080`).
BIND ?= 127.0.0.1:3000
URL  := http://$(BIND)

# Run backend from its own dir so relative paths resolve:
#   data/  (git-ignored SQLite dir)  and  ../frontend/dist (debug asset reads).
CARGO := cd $(BACKEND) &&
BUN   := cd $(FRONTEND) &&

.DEFAULT_GOAL := help

# ---- meta -------------------------------------------------------------------
.PHONY: help
help: ## Show this help
	@echo "Zenithar — make targets:"
	@grep -hE '^[a-zA-Z0-9_-]+:.*?## ' $(MAKEFILE_LIST) \
		| awk 'BEGIN{FS=":.*?## "}{printf "  \033[1m%-16s\033[0m %s\n", $$1, $$2}'

# ---- full build -------------------------------------------------------------
.PHONY: build
build: fe-build be-build ## Full release build: frontend embedded into the binary
	@echo "==> binary: $(BACKEND)/target/release/zenithar-backend (frontend embedded)"

.PHONY: run
run: fe-build ## Build frontend, then run the release server (serves embedded site)
	@printf '\n  \033[1mZenithar\033[0m → %s\n\n' '$(URL)'
	$(CARGO) set -a; [ -f .env ] && . ./.env; set +a; ZENITHAR_BIND=$(BIND) cargo run --release

.PHONY: dev
dev: fe-build ## Run the debug server (reads frontend/dist from disk; pair with `make fe-dev`)
	@printf '\n  \033[1mZenithar dev\033[0m → %s\n\n' '$(URL)'
	$(CARGO) set -a; [ -f .env ] && . ./.env; set +a; ZENITHAR_BIND=$(BIND) cargo run

# ---- backend (Rust) ---------------------------------------------------------
.PHONY: be-build
be-build: ## Build backend (release)
	$(CARGO) cargo build --release

.PHONY: be-test
be-test: ## Run backend tests
	$(CARGO) cargo test

.PHONY: be-lint
be-lint: ## Lint backend (clippy, warnings = errors)
	$(CARGO) cargo clippy --all-targets -- -D warnings

.PHONY: be-fmt
be-fmt: ## Format backend code
	$(CARGO) cargo fmt

.PHONY: be-fmt-check
be-fmt-check: ## Check backend formatting (CI)
	$(CARGO) cargo fmt --check

# ---- frontend (bun) ---------------------------------------------------------
.PHONY: fe-install
fe-install: ## Install frontend deps
	$(BUN) bun install

.PHONY: fe-build
fe-build: fe-install ## Build frontend bundle into frontend/dist
	$(BUN) bun run build

.PHONY: fe-dev
fe-dev: fe-install ## Rebuild frontend on change (run alongside `make dev`)
	@printf '\n  watching frontend — open the app at %s (needs `make dev` running)\n\n' '$(URL)'
	$(BUN) bun run dev

.PHONY: fe-typecheck
fe-typecheck: fe-install ## Type-check frontend
	$(BUN) bun run typecheck

# ---- desktop app (Tauri) ----------------------------------------------------
APP := app
# Android (macOS host): SDK location + the NDK version to auto-install if missing.
ANDROID_SDK ?= $(HOME)/Library/Android/sdk
NDK_VERSION ?= 27.2.12479018
ADB ?= $(ANDROID_SDK)/platform-tools/adb
ANDROID_DEBUG_APK = $(APP)/src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk

.PHONY: app-deps
app-deps: ## Install the Tauri CLI (one-time)
	cd $(APP) && bun install

.PHONY: app-icons
app-icons: app-deps ## Regenerate the icon set from app/app-icon.png
	cd $(APP) && bun run tauri icon app-icon.png

# Tauri reads the macOS Info.plist only from src-tauri/, so stage ours (the source
# of truth lives in platforms/macos/; the copy is a build artifact, git-ignored).
.PHONY: app-mac
app-mac: fe-build app-icons ## Build the macOS desktop app (.app/.dmg) — macOS only
	cd $(APP) && cp platforms/macos/Info.plist src-tauri/Info.plist && bun run tauri build

.PHONY: app-dev
app-dev: app-deps ## Run the desktop app in a dev window (loads the live server)
	cd $(APP) && cp platforms/macos/Info.plist src-tauri/Info.plist && bun run tauri dev

.PHONY: app-android
app-android: fe-build app-icons ## Build the Android debug .apk (arm64 only — for a phone); auto-resolves JDK17/SDK/NDK on macOS
	@set -e; \
	SDK="$(ANDROID_SDK)"; \
	JH="$$(/usr/libexec/java_home -v 17 2>/dev/null)"; \
	[ -n "$$JH" ] || { echo "✗ JDK 17 not found — brew install --cask temurin@17"; exit 1; }; \
	[ -d "$$SDK" ] || { echo "✗ Android SDK not at $$SDK (override: make app-android ANDROID_SDK=/path)"; exit 1; }; \
	NDK="$$(ls -d $$SDK/ndk/* 2>/dev/null | sort -V | tail -1)"; \
	if [ -z "$$NDK" ]; then \
	  SM="$$(command -v sdkmanager 2>/dev/null || true)"; \
	  [ -n "$$SM" ] || SM="$$(ls $$SDK/cmdline-tools/*/bin/sdkmanager 2>/dev/null | head -1)"; \
	  if [ -n "$$SM" ]; then \
	    echo "→ NDK missing; installing ndk;$(NDK_VERSION) into $$SDK …"; \
	    yes | JAVA_HOME="$$JH" "$$SM" --sdk_root="$$SDK" "ndk;$(NDK_VERSION)" >/dev/null; \
	    NDK="$$SDK/ndk/$(NDK_VERSION)"; \
	  else \
	    echo "✗ No NDK and no sdkmanager to install it. Either:"; \
	    echo "    • Android Studio → SDK Manager → SDK Tools → check 'NDK (Side by side)' → Apply, OR"; \
	    echo "    • brew install --cask android-commandlinetools   (then re-run; make will install the NDK)"; \
	    exit 1; \
	  fi; \
	fi; \
	echo "→ JAVA_HOME=$$JH"; echo "→ ANDROID_HOME=$$SDK"; echo "→ NDK_HOME=$$NDK"; \
	cd $(APP) && \
	  JAVA_HOME="$$JH" ANDROID_HOME="$$SDK" NDK_HOME="$$NDK" bun run tauri android init && \
	  perl -pi -e 's/compileSdk\s*=\s*\d+/compileSdk = 36/; s/targetSdk\s*=\s*\d+/targetSdk = 36/' src-tauri/gen/android/app/build.gradle.kts && \
	  bash platforms/android/patch.sh && \
	  RUSTFLAGS="-C strip=debuginfo" JAVA_HOME="$$JH" ANDROID_HOME="$$SDK" NDK_HOME="$$NDK" \
	    bun run tauri android build --apk --debug --target aarch64; \
	  echo "→ APK: $(ANDROID_DEBUG_APK)"

.PHONY: app-android-install
app-android-install: app-android ## Build the arm64 debug .apk and install it on the connected device (adb)
	$(ADB) install -r "$(ANDROID_DEBUG_APK)"
	@echo "→ installed com.feziv.zenithar"

# Signed RELEASE build (small + installable). Reads signing secrets from a
# git-ignored `.env` at the repo root (see .env.example). Same keystore as CI.
.PHONY: app-android-release
app-android-release: fe-build app-icons ## Signed RELEASE .apk — reads ANDROID_KEYSTORE/_PASSWORD/_KEY_PASSWORD from .env
	@set -e; \
	[ -f .env ] && { set -a; . ./.env; set +a; } || { echo "✗ no .env (copy .env.example → .env, fill the ANDROID_* keys)"; exit 1; }; \
	KS="$$ANDROID_KEYSTORE"; ALIAS="$${ANDROID_KEY_ALIAS:-zenithar}"; \
	[ -n "$$KS" ] || { echo "✗ set ANDROID_KEYSTORE (path to zenithar.jks) in .env"; exit 1; }; \
	case "$$KS" in /*) ;; *) KS="$$PWD/$$KS";; esac; \
	[ -f "$$KS" ] || { echo "✗ keystore not found: $$KS"; exit 1; }; \
	[ -n "$$ANDROID_KEYSTORE_PASSWORD" ] || { echo "✗ set ANDROID_KEYSTORE_PASSWORD in .env"; exit 1; }; \
	[ -n "$$ANDROID_KEY_PASSWORD" ] || { echo "✗ set ANDROID_KEY_PASSWORD in .env"; exit 1; }; \
	SDK="$(ANDROID_SDK)"; \
	JH="$$(/usr/libexec/java_home -v 17 2>/dev/null)"; \
	[ -n "$$JH" ] || { echo "✗ JDK 17 not found — brew install --cask temurin@17"; exit 1; }; \
	NDK="$$(ls -d $$SDK/ndk/* 2>/dev/null | sort -V | tail -1)"; \
	[ -n "$$NDK" ] || { echo "✗ no NDK — run 'make app-android' once to install it"; exit 1; }; \
	echo "→ signed release with $$KS (alias $$ALIAS)"; \
	cd $(APP) && \
	  JAVA_HOME="$$JH" ANDROID_HOME="$$SDK" NDK_HOME="$$NDK" bun run tauri android init && \
	  perl -pi -e 's/compileSdk\s*=\s*\d+/compileSdk = 36/; s/targetSdk\s*=\s*\d+/targetSdk = 36/' src-tauri/gen/android/app/build.gradle.kts && \
	  bash platforms/android/patch.sh && \
	  rm -rf src-tauri/gen/android/app/build/outputs/apk && \
	  RUSTFLAGS="-C strip=symbols" JAVA_HOME="$$JH" ANDROID_HOME="$$SDK" NDK_HOME="$$NDK" \
	    bun run tauri android build --apk --split-per-abi && \
	  AS="$$(ls -d $$SDK/build-tools/*/apksigner | sort -V | tail -1)"; \
	  for apk in src-tauri/gen/android/app/build/outputs/apk/*/release/*-unsigned.apk; do \
	    JAVA_HOME="$$JH" "$$AS" sign --ks "$$KS" --ks-pass "pass:$$ANDROID_KEYSTORE_PASSWORD" \
	      --ks-key-alias "$$ALIAS" --key-pass "pass:$$ANDROID_KEY_PASSWORD" \
	      --out "$${apk%-unsigned.apk}.apk" "$$apk" && rm -f "$$apk"; \
	  done; \
	  echo "→ signed APKs:"; ls -lh src-tauri/gen/android/app/build/outputs/apk/*/release/*.apk

# ---- aggregate --------------------------------------------------------------
.PHONY: e2e
e2e: fe-build ## Run local end-to-end tests (ephemeral server on a temp DB)
	$(CARGO) cargo build
	bash tests/run.sh

.PHONY: test
test: be-test fe-typecheck ## Run all tests / checks

.PHONY: lint
lint: be-lint be-fmt-check fe-typecheck ## Lint everything

.PHONY: fmt
fmt: be-fmt ## Format everything

# ---- housekeeping -----------------------------------------------------------
.PHONY: kill
kill: ## Kill stray dev/test processes (servers, Playwright browsers, leftover bun)
	@echo "killing stray zenithar/test processes…"
	-@pkill -9 -f 'target/debug/zenithar-backend' 2>/dev/null || true
	-@pkill -9 -f 'target/release/zenithar-backend' 2>/dev/null || true
	-@pkill -9 -f 'chrome-headless-shell' 2>/dev/null || true
	-@pkill -9 -f 'ms-playwright' 2>/dev/null || true
	-@pkill -9 -f 'tests/e2e.spec.ts' 2>/dev/null || true
	@echo "done."

.PHONY: db-reset
db-reset: ## Delete the local SQLite database
	rm -f $(BACKEND)/data/*.db $(BACKEND)/data/*.db-wal $(BACKEND)/data/*.db-shm

.PHONY: clean
clean: ## Remove all build artifacts (keeps the data/ dir)
	$(CARGO) cargo clean
	$(BUN) bun run clean
