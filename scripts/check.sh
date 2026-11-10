#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo llvm-cov --workspace --all-targets --fail-under-lines 80

# cfg(windows) code is invisible to a host-target lint, so lint it from a
# Windows target too. Skipped when the cross toolchain is not installed.
if command -v cargo-xwin >/dev/null && rustup target list --installed | grep -qx x86_64-pc-windows-msvc; then
  cargo xwin clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
else
  echo "note: skipping Windows cross-check (cargo-xwin or the msvc target is missing)"
fi

tests/nvim/run.sh
tests/qutebrowser/run.sh
tests/zathura/run.sh
tests/firefox/run.sh
