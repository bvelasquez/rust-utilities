SHELL := /bin/bash
PREFIX ?= $(HOME)/.cargo/bin
BIN := target/release/pi-commander

.PHONY: build release test install uninstall clean

build:
	cargo build

release:
	cargo build --release

test:
	cargo test

install: release
	install -d $(PREFIX)
	install -m 0755 $(BIN) $(PREFIX)/pi-commander
	@echo "installed $(PREFIX)/pi-commander"

uninstall:
	rm -f $(PREFIX)/pi-commander

clean:
	cargo clean
