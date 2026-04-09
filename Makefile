.PHONY: build dev test clean install lint fmt check doctor

# ============================================================
# Centaur Psicode — Build Orchestration
# ============================================================

RUST_DIR   := agent-core
PYTHON_DIR := agent-brain
TS_DIR     := agent-integrations
BINARY     := $(RUST_DIR)/target/release/agent

# Pin Python: use homebrew Python 3.10. Override with:
#   make dev PYTHON=python3
PYTHON     ?= /opt/homebrew/opt/python@3.10/libexec/bin/python3

# ---- Build -----------------------------------------------

build: build-rust build-python build-ts
	@echo "✓ All components built"

build-rust:
	@echo "→ Building agent-core (Rust)..."
	cd $(RUST_DIR) && cargo build --release

build-python:
	@echo "→ Installing agent-brain (Python)..."
	cd $(PYTHON_DIR) && pip install -e . --quiet

build-ts:
	@echo "→ Building agent-integrations (TypeScript)..."
	cd $(TS_DIR) && npm install --silent && npm run build

# ---- Development -----------------------------------------

dev: deps
	@echo "→ Cleaning up stale processes..."
	@-pkill -9 -f "agent_brain.ipc_server" 2>/dev/null || true
	@-rm -f /tmp/agent-ipc.sock 2>/dev/null || true
	@find $(PYTHON_DIR) -name "__pycache__" -type d -exec rm -rf {} + 2>/dev/null || true
	@sleep 1
	@$(PYTHON) -c "import msgpack; print('  msgpack OK')" || { echo "ERROR: msgpack not installed. Run: $(PYTHON) -m pip install msgpack"; exit 1; }
	@echo "→ Starting agent-brain IPC server ($(PYTHON))..."
	@cd $(PYTHON_DIR) && $(PYTHON) -m agent_brain.ipc_server & \
	BRAIN_PID=$$!; \
	trap "kill $$BRAIN_PID 2>/dev/null; rm -f /tmp/agent-ipc.sock; exit" INT TERM EXIT; \
	sleep 1; \
	echo "→ Starting agent-core (debug)..."; \
	cd $(RUST_DIR) && cargo run; \
	echo "→ Shutting down agent-brain (PID $$BRAIN_PID)..."; \
	kill $$BRAIN_PID 2>/dev/null; \
	wait $$BRAIN_PID 2>/dev/null; \
	rm -f /tmp/agent-ipc.sock; \
	echo "→ Clean shutdown complete."

dev-rust:
	cd $(RUST_DIR) && cargo run

dev-python:
	cd $(PYTHON_DIR) && $(PYTHON) -m agent_brain.ipc_server

dev-gui: deps
	@echo "→ Starting Centaur Psicode Web GUI..."
	@$(PYTHON) -c "import websockets" 2>/dev/null || ( \
		echo "→ Installing websockets..."; \
		$(PYTHON) -m pip install --quiet websockets; \
	)
	cd $(PYTHON_DIR) && $(PYTHON) ../gui/server.py

# ---- Dependency check (auto-install missing Python packages) ----

deps:
	@$(PYTHON) -c "import msgpack, anthropic" 2>/dev/null || ( \
		echo "→ Installing missing Python dependencies for $$($(PYTHON) --version) at $$(which $(PYTHON))..."; \
		$(PYTHON) -m pip install --quiet msgpack anthropic pydantic pytest; \
	)

# ---- Test ------------------------------------------------

test: test-rust test-python test-ts

test-rust:
	@echo "→ Running Rust tests..."
	cd $(RUST_DIR) && cargo test

test-python:
	@echo "→ Running Python tests..."
	cd $(PYTHON_DIR) && $(PYTHON) -m pytest tests/ -v

test-ts:
	@echo "→ Running TypeScript tests..."
	cd $(TS_DIR) && npm test

test-e2e:
	@echo "→ Running end-to-end IPC test..."
	cd $(PYTHON_DIR) && $(PYTHON) -m agent_brain.ipc_server &
	@BRAIN_PID=$$!; \
	trap "kill $$BRAIN_PID 2>/dev/null" EXIT; \
	sleep 1; \
	cd $(RUST_DIR) && cargo test ipc_integration -- --ignored; \
	kill $$BRAIN_PID 2>/dev/null

# ---- Quality ---------------------------------------------

lint: lint-rust lint-python lint-ts

lint-rust:
	cd $(RUST_DIR) && cargo clippy -- -D warnings

lint-python:
	cd $(PYTHON_DIR) && $(PYTHON) -m ruff check .

lint-ts:
	cd $(TS_DIR) && npm run lint

fmt: fmt-rust fmt-python fmt-ts

fmt-rust:
	cd $(RUST_DIR) && cargo fmt

fmt-python:
	cd $(PYTHON_DIR) && $(PYTHON) -m ruff format .

fmt-ts:
	cd $(TS_DIR) && npm run fmt

check:
	cd $(RUST_DIR) && cargo check

# ---- Install (macOS universal binary) --------------------

install: build-rust
	@echo "→ Installing agent binary to /usr/local/bin..."
	install -m 755 $(BINARY) /usr/local/bin/agent
	@echo "✓ Installed: /usr/local/bin/agent"

install-universal:
	@echo "→ Building universal macOS binary..."
	cd $(RUST_DIR) && cargo build --release --target aarch64-apple-darwin
	cd $(RUST_DIR) && cargo build --release --target x86_64-apple-darwin
	lipo -create -output $(BINARY)-universal \
		$(RUST_DIR)/target/aarch64-apple-darwin/release/agent \
		$(RUST_DIR)/target/x86_64-apple-darwin/release/agent
	install -m 755 $(BINARY)-universal /usr/local/bin/agent
	@echo "✓ Universal binary installed"

# ---- Diagnostics -----------------------------------------

doctor:
	@echo "=== Centaur Psicode Doctor ==="
	@echo "--- Rust ---"
	@rustc --version 2>/dev/null || echo "  ✗ Rust not found — install from https://rustup.rs"
	@cargo --version 2>/dev/null || echo "  ✗ Cargo not found"
	@echo "--- Python ---"
	@$(PYTHON) --version 2>/dev/null || echo "  ✗ Python3 not found"
	@echo "  Python path: $$(which $(PYTHON))"
	@$(PYTHON) -c "import anthropic" 2>/dev/null && echo "  ✓ anthropic SDK" || echo "  ✗ anthropic not installed"
	@$(PYTHON) -c "import msgpack"   2>/dev/null && echo "  ✓ msgpack"        || echo "  ✗ msgpack not installed"
	@echo "--- Node/TS ---"
	@node --version 2>/dev/null || echo "  ✗ Node not found"
	@echo "--- Environment ---"
	@test -f .env && echo "  ✓ .env present" || echo "  ✗ .env missing — copy from .env.example"
	@test -n "$$ANTHROPIC_API_KEY" && echo "  ✓ ANTHROPIC_API_KEY set" || echo "  ✗ ANTHROPIC_API_KEY not set"
	@echo "--- IPC socket ---"
	@test -S /tmp/agent-ipc.sock && echo "  ✓ IPC server running" || echo "  - IPC server not running (start with: make dev-python)"

# ---- Clean -----------------------------------------------

clean:
	cd $(RUST_DIR) && cargo clean
	cd $(PYTHON_DIR) && rm -rf build dist *.egg-info __pycache__
	cd $(TS_DIR)     && rm -rf node_modules dist
	@echo "✓ Clean"
