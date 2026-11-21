#!/bin/bash
# Checks the mod and runs the rules' tests against the API crate in wayfarer_stones/.horizon/sdk/
# (the SDK `cargo horizon-mod` fetches, or a link to a Horizon checkout of the pinned commit).
# Cargo.lock stays as `horizon-launcher native-lock` wrote it: registry pins only.
#   tools/check.sh          cargo check, then cargo test
set -u
cd "$(dirname "$0")/../wayfarer_stones" || exit 1
cp Cargo.lock "${TMPDIR:-/tmp}/wayfarer-Cargo.lock"
# Put back even when the run is interrupted.
trap 'cp "${TMPDIR:-/tmp}/wayfarer-Cargo.lock" Cargo.lock' EXIT
export RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN:-stable-x86_64-pc-windows-gnu}
cargo check --message-format short 2>&1 | grep -E '^(src|error|warning)'
cargo test 2>&1 | grep -E '^test result|FAILED|panicked'
exit "${PIPESTATUS[0]}"
