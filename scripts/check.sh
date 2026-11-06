#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo llvm-cov --workspace --all-targets --fail-under-lines 80
tests/nvim/run.sh
