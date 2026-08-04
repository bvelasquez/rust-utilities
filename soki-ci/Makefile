.PHONY: help build install test clean

help:
	@echo "soki-ci — build and install"
	@echo "  make build    Release binary"
	@echo "  make install  Install to ~/.cargo/bin"
	@echo "  make test     cargo test"

build:
	@./scripts/build.sh

install:
	@./scripts/install.sh

test:
	@./scripts/test.sh

clean:
	cargo clean
