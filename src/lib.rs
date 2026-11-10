//! Docent library: configuration, profiles, and provider clients.
//!
//! The CLI lives in `main.rs` and the Firefox bridge in
//! `bin/docent-native-host.rs`. This crate exposes the pieces needed for
//! testing and for the JSON contracts used by the adapters.
//!
//! The library follows the CLI and is not a stable interface: items can change
//! in any release. The supported contracts are the configuration and profile
//! formats, the request and event JSON on stdin and stdout, and the command
//! line itself, all documented in `docs/` of the source repository.

pub mod cache;
pub mod cli;
pub mod config;
pub mod context;
pub mod defaults;
pub mod epub;
pub mod error;
pub mod input;
pub mod markdown;
pub mod native_host;
pub mod pdf;
pub mod profile;
pub mod prompt;
pub mod provider;
pub mod request;
pub mod storage;
pub mod viewer;

pub use error::DocentError;
