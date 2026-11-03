.PHONY: help build install test clean web-install web-build web-dev web-typecheck

help:
	@echo "soki-ci — build and install"
	@echo "  make build         Release binary with embedded web UI"
	@echo "  make install       Install to ~/.cargo/bin"
	@echo "  make test          cargo test + web typecheck"
	@echo "  make web-install   Install web dependencies"
	@echo "  make web-build     Build the React web UI"
	@echo "  make web-dev       Vite dev server (proxy to 127.0.0.1:9847)"

build:
	@./scripts/build.sh

install:
	@./scripts/install.sh

test:
	@./scripts/test.sh

web-install:
	@cd web && (pnpm install || npx --yes pnpm@10 install)

web-build:
	@./scripts/web-build.sh

web-dev:
	@./scripts/web-dev.sh

web-typecheck:
	@cd web && (pnpm run typecheck || npx --yes pnpm@10 run typecheck)

clean:
	cargo clean
	rm -rf web/node_modules web/dist
