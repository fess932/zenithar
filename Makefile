# Zenithar — single entry point for all dev/build/test/lint tasks.
# Backend: Rust (cargo). Frontend: TypeScript (bun). Run `make` for the list.

BACKEND  := backend
FRONTEND := frontend

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
	$(CARGO) cargo run --release

.PHONY: dev
dev: fe-build ## Run the debug server (reads frontend/dist from disk; pair with `make fe-dev`)
	$(CARGO) cargo run

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
	$(BUN) bun run dev

.PHONY: fe-typecheck
fe-typecheck: fe-install ## Type-check frontend
	$(BUN) bun run typecheck

# ---- aggregate --------------------------------------------------------------
.PHONY: test
test: be-test fe-typecheck ## Run all tests / checks

.PHONY: lint
lint: be-lint be-fmt-check fe-typecheck ## Lint everything

.PHONY: fmt
fmt: be-fmt ## Format everything

# ---- housekeeping -----------------------------------------------------------
.PHONY: db-reset
db-reset: ## Delete the local SQLite database
	rm -f $(BACKEND)/data/*.db $(BACKEND)/data/*.db-wal $(BACKEND)/data/*.db-shm

.PHONY: clean
clean: ## Remove all build artifacts (keeps the data/ dir)
	$(CARGO) cargo clean
	$(BUN) bun run clean
