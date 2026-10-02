#!/usr/bin/env bash
# The gate, with every exit code checked (never a piped summary). Run from anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
./scripts/check-boundary.sh
cargo deny check licenses
echo "GATE GREEN"
