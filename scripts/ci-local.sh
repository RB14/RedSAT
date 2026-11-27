#!/usr/bin/env bash
# Runs, locally, the same checks as CI (.github/workflows/ci.yml), stopping at the first
# failure. The pre-push hook (.githooks/pre-push) runs it before every push, so a push that would
# fail CI does not leave the machine. Keep the two in sync when the workflow changes.
#
# Usage: scripts/ci-local.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
[ -f "$HOME/.cargo/env" ] && source "$HOME/.cargo/env"

step() {
    local name="$1"
    shift
    local started=$SECONDS
    printf '▶ %s\n' "$name"
    if ! "$@"; then
        printf '✗ %s failed\n' "$name" >&2
        exit 1
    fi
    printf '✓ %s (%ss)\n' "$name" $((SECONDS - started))
}

step "formatting" cargo fmt --all --check
step "lints" cargo clippy --quiet --workspace --all-targets -- -D warnings
step "tests" cargo test --quiet --workspace
step "WebAssembly build" cargo build --quiet --release --target wasm32-unknown-unknown -p redsat-wasm
step "playground tests" bash -c 'cd playground && npm test --silent'
printf 'All CI checks passed.\n'
