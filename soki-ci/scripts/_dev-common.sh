_dev_root() {
    local here
    here="$(cd "$(dirname "${BASH_SOURCE[1]:-${BASH_SOURCE[0]}}")/.." && pwd)"
    printf '%s' "$here"
}

dev_cd_root() {
    cd "$(_dev_root)"
}

dev_cargo() {
    cargo "$@"
}
