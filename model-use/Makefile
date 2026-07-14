.PHONY: help build install test publish clean setup-sccache

help:
	@echo "model-use — build and install"
	@echo ""
	@echo "Targets:"
	@echo "  make build          Build release binary (target/release/model-use)"
	@echo "  make install        Build and install latest to ~/.cargo/bin"
	@echo "  make test           Run cargo test"
	@echo "  make publish        Publish to crates.io (requires cargo login)"
	@echo "  make clean          Remove target/ build artifacts"
	@echo "  make setup-sccache  Install sccache + lld (one-time, recommended)"
	@echo ""
	@echo "Examples:"
	@echo "  make setup-sccache && make install"
	@echo "  ./target/release/model-use capabilities --json"

build:
	@./scripts/build.sh

install:
	@./scripts/install.sh

test:
	@./scripts/test.sh

publish:
	@./scripts/publish.sh

clean:
	cargo clean

setup-sccache:
	@./scripts/setup-sccache.sh
