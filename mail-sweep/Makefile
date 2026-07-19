.PHONY: help build install test clean run sync process

BINARY := target/release/mail-sweep
CARGO  := cargo

help:
	@echo "mail-sweep — build, install, and run"
	@echo ""
	@echo "Targets:"
	@echo "  make build     Build release binary ($(BINARY))"
	@echo "  make install   Build and install to ~/.cargo/bin"
	@echo "  make test      Run cargo test"
	@echo "  make clean     Remove target/ build artifacts"
	@echo "  make run       Build (if needed) and launch interactive TUI"
	@echo "  make sync      Build (if needed) and sync mail --json"
	@echo "  make process   Build (if needed) and classify pending mail (dry-run)"
	@echo ""
	@echo "Examples:"
	@echo "  make install && mail-sweep"
	@echo "  make run"
	@echo "  mail-sweep capabilities --json"

build:
	$(CARGO) build --release
	@echo ""
	@echo "Built: $(BINARY)"

install: build
	$(CARGO) install --path . --force
	@echo ""
	@command -v mail-sweep >/dev/null 2>&1 && echo "Installed: $$(command -v mail-sweep)" || echo "Installed. Ensure ~/.cargo/bin is on your PATH."

test:
	$(CARGO) test

clean:
	$(CARGO) clean

run: build
	$(BINARY)

sync: build
	$(BINARY) sync --json

process: build
	$(BINARY) process --dry-run --json
