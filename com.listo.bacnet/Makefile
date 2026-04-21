# com.listo.bacnet
#
# Usage:
#   make                    # build UI + process (default)
#   make build              # same
#   make ui                 # build just the UI bundle
#   make process            # build just the native process binary
#   make edge               # build + stage into agent/dev/edge-blocks/ + hot-reload
#   make cloud              # build + stage into agent/dev/cloud-blocks/ + hot-reload
#   make reload             # POST /api/v1/blocks/reload (no rebuild)
#   make uninstall          # remove symlinks from both agent blocks dirs
#   make clean              # remove dist/, ui/, target/

BLOCK_DIR   := $(abspath $(CURDIR))
BLOCK_ID    := com.listo.bacnet
WORKSPACE   := $(abspath $(CURDIR)/../..)
AGENT_DEV   := $(WORKSPACE)/agent/dev
DIST        := $(BLOCK_DIR)/dist

EDGE_URL  := http://localhost:8082
CLOUD_URL := http://localhost:8081

.PHONY: build ui process edge cloud install-edge install-cloud uninstall run clean reload reload-edge reload-cloud help

build: ui process ## Build UI bundle + native process binary
	@echo "✓ built $(BLOCK_ID) — UI at ui/, process at dist/process"

# -----------------------------------------------------------------------------
# UI bundle (Module Federation remote)
# -----------------------------------------------------------------------------

ui: $(BLOCK_DIR)/ui/remoteEntry.js ## Build the Module-Federation UI bundle

$(BLOCK_DIR)/ui/remoteEntry.js: ui-src/src/*.tsx ui-src/src/*.ts ui-src/package.json ui-src/rsbuild.config.ts
	cd ui-src && pnpm build

# -----------------------------------------------------------------------------
# Process-block binary (native)
# -----------------------------------------------------------------------------

process: $(DIST)/process ## Build the native process-block binary

$(DIST)/process: process/src/main.rs process/Cargo.toml kinds/*.yaml
	cd process && cargo build --release
	mkdir -p $(DIST)
	cp process/target/release/listo-bacnet $(DIST)/process

# -----------------------------------------------------------------------------
# Stage into an agent's blocks dir
# -----------------------------------------------------------------------------

edge: install-edge reload-edge ## Build + stage into edge-blocks/ + hot-reload if running
cloud: install-cloud reload-cloud ## Build + stage into cloud-blocks/ + hot-reload if running

install-edge: build
	@mkdir -p $(AGENT_DEV)/edge-blocks
	ln -sfn $(BLOCK_DIR) $(AGENT_DEV)/edge-blocks/$(BLOCK_ID)
	@echo "✓ staged → $(AGENT_DEV)/edge-blocks/$(BLOCK_ID)"

install-cloud: build
	@mkdir -p $(AGENT_DEV)/cloud-blocks
	ln -sfn $(BLOCK_DIR) $(AGENT_DEV)/cloud-blocks/$(BLOCK_ID)
	@echo "✓ staged → $(AGENT_DEV)/cloud-blocks/$(BLOCK_ID)"

uninstall: ## Remove this block's symlinks from both agent blocks dirs
	rm -f $(AGENT_DEV)/edge-blocks/$(BLOCK_ID)
	rm -f $(AGENT_DEV)/cloud-blocks/$(BLOCK_ID)
	@echo "✓ unstaged"

run: install-edge ## Build and stage (does NOT start the dev server)
	@echo "  ✓ built + staged — ready to load."
	@echo "  If edge is running: make reload"
	@echo "  Otherwise:          mani run dev-edge --projects agent"

reload: reload-edge ## Alias for reload-edge (the common case)

reload-edge: ## POST /api/v1/blocks/reload to the edge agent (silent if not running)
	@curl -fsS -X POST $(EDGE_URL)/api/v1/blocks/reload  >/dev/null 2>&1 && echo "✓ reloaded edge ($(EDGE_URL))"  || echo "· edge agent not running — skipping reload"

reload-cloud: ## POST /api/v1/blocks/reload to the cloud agent (silent if not running)
	@curl -fsS -X POST $(CLOUD_URL)/api/v1/blocks/reload >/dev/null 2>&1 && echo "✓ reloaded cloud ($(CLOUD_URL))" || echo "· cloud agent not running — skipping reload"

clean: ## Remove dist/, ui/, target/
	rm -rf $(DIST) $(BLOCK_DIR)/ui $(BLOCK_DIR)/process/target
	@echo "✓ cleaned"

help: ## Show this help
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z_-]+:.*?## / {printf "  %-18s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

.DEFAULT_GOAL := build
