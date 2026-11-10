# AGENTS.md

Project-specific instructions for coding agents. The shared workflow rules in the parent agent configuration still apply.

## Layout

- `src/`: Rust library and the `docent` binary.
- `src/bin/docent-native-host.rs`: Firefox native messaging host.
- `plugins/nvim/`, `plugins/qutebrowser/`, `plugins/firefox/`: in-repository adapters.
- `plugins/zathura/`: external zathura adapter.
- `defaults/`: shipped config, profiles, and example answer CSS.
- `docs/`: durable reference. Start at `docs/index.md`.
- `tests/`: Rust integration tests and adapter test suites.

## Commands

Run `scripts/check.sh` before considering a change done. It runs formatting, clippy, Rust tests, coverage, the Windows cross-check, and the adapter tests.

Individual checks:

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`
- `cargo llvm-cov --workspace --all-targets --fail-under-lines 80`
- `cargo xwin clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
- `tests/nvim/run.sh`
- `tests/qutebrowser/run.sh`
- `tests/zathura/run.sh`
- `tests/firefox/run.sh`

The Windows check needs `cargo-xwin` on `PATH`, the `x86_64-pc-windows-msvc` rustup target, and `lld-link`, which may be a symlink to the toolchain's `rust-lld`. Without them a host-target clippy cannot see warnings in `cfg(windows)` code, which is what CI lints on `windows-latest`.

## Documentation

Update `README.md` for user-facing changes and `docs/` for behavior, configuration, architecture, or module changes. Keep `docs/index.md` in sync and describe only shipped behavior. Future work belongs in the local agent_artifacts backlog file.
