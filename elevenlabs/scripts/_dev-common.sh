# Shared helpers for local dev scripts. Source from other scripts; do not run directly.

_dev_root() {
    local here
    here="$(cd "$(dirname "${BASH_SOURCE[1]:-${BASH_SOURCE[0]}}")/.." && pwd)"
    printf '%s' "$here"
}

dev_cd_root() {
    cd "$(_dev_root)"
}

dev_load_env() {
    dev_cd_root
    if [[ -f .env ]]; then
        set -a
        # shellcheck disable=SC1091
        source .env
        set +a
    fi
    dev_enable_sccache
}

# Use sccache when installed (brew install sccache / make setup-sccache).
dev_enable_sccache() {
    if [[ -n "${RUSTC_WRAPPER:-}" ]]; then
        return 0
    fi
    if command -v sccache >/dev/null 2>&1; then
        export RUSTC_WRAPPER=sccache
        export SCCACHE_IDLE_TIMEOUT="${SCCACHE_IDLE_TIMEOUT:-0}"
        export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
    fi
}

dev_cargo() {
    dev_load_env
    cargo "$@"
}
