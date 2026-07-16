.PHONY: help build install test clean run watch scan analyze

BINARY := target/release/disk-sweep
CARGO  := cargo

help:
	@echo "disk-sweep — build, install, and run"
	@echo ""
	@echo "Targets:"
	@echo "  make build     Build release binary ($(BINARY))"
	@echo "  make install   Build and install to ~/.cargo/bin"
	@echo "  make test      Run cargo test"
	@echo "  make clean     Remove target/ build artifacts"
	@echo "  make run       Build (if needed) and launch interactive TUI"
	@echo "  make watch     Build (if needed) and launch watch dashboard"
	@echo "  make scan      Build (if needed) and run scan --json"
	@echo "  make analyze   Build (if needed) and run analyze --json"
	@echo ""
	@echo "Examples:"
	@echo "  make install && disk-sweep"
	@echo "  make run"
	@echo "  ./target/release/disk-sweep capabilities --json"

build:
	$(CARGO) build --release
	@echo ""
	@echo "Built: $(BINARY)"

install: build
	$(CARGO) install --path . --force
	@echo ""
	@command -v disk-sweep >/dev/null 2>&1 && echo "Installed: $$(command -v disk-sweep)" || echo "Installed. Ensure ~/.cargo/bin is on your PATH."

test:
	$(CARGO) test

clean:
	$(CARGO) clean

run: build
	$(BINARY)

watch: build
	$(BINARY) watch

scan: build
	$(BINARY) scan --json

analyze: build
	$(BINARY) analyze --json
