# Install to /usr/local when writable, otherwise fall back to ~/.local.
# Explicitly override with: make install PREFIX=/some/prefix
ifeq ($(origin PREFIX),undefined)
  ifeq ($(shell test -w /usr/local/bin && echo yes),yes)
    PREFIX := /usr/local
  else
    PREFIX := $(HOME)/.local
  endif
endif
BINDIR ?= $(PREFIX)/bin
DESTDIR ?=
CARGO ?= cargo

BIN     := git-sweep
RELEASE := target/release/$(BIN)

.PHONY: all build install uninstall test check fmt clippy clean help

all: build

## Build the release binary
build:
	$(CARGO) build --release

## Install the binary into $(BINDIR) (default /usr/local/bin)
install: build
	install -d "$(DESTDIR)$(BINDIR)"
	install -m 0755 "$(RELEASE)" "$(DESTDIR)$(BINDIR)/$(BIN)"
	@echo "Installed $(BIN) to $(DESTDIR)$(BINDIR)/$(BIN)"
	@case ":$$PATH:" in *":$(BINDIR):"*) ;; \
	  *) echo "Warning: $(BINDIR) is not in your PATH. Add it, e.g.:"; \
	     echo "  echo 'export PATH=\"$(BINDIR):$$PATH\"' >> ~/.bashrc" ;; \
	esac

## Remove the installed binary
uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/$(BIN)"
	@echo "Removed $(DESTDIR)$(BINDIR)/$(BIN)"

## Run tests
test:
	$(CARGO) test

## Type-check without producing a binary
check:
	$(CARGO) check

fmt:
	$(CARGO) fmt

clippy:
	$(CARGO) clippy --all-targets -- -D warnings

## Remove build artifacts
clean:
	$(CARGO) clean

help:
	@echo "Targets:"
	@echo "  make build      Build release binary ($(RELEASE))"
	@echo "  make install    Install to $(BINDIR) (override: make install PREFIX=~/.local)"
	@echo "                  Uses /usr/local if writable, else ~/.local"
	@echo "  make uninstall  Remove installed binary"
	@echo "  make test       Run tests"
	@echo "  make check      cargo check"
	@echo "  make fmt        cargo fmt"
	@echo "  make clippy     cargo clippy -D warnings"
	@echo "  make clean      cargo clean"
