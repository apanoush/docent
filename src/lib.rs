//! Docent library: configuration, profiles, and provider clients.
//!
//! The CLI lives in `main.rs` and the Firefox bridge in
//! `bin/docent-native-host.rs`. This crate exposes the pieces needed for
//! testing and for the JSON contracts used by the adapters.

pub mod cache;
pub mod cli;
pub mod config;
pub mod context;
pub mod defaults;
pub mod error;
pub mod history;
pub mod markdown;
pub mod native_host;
pub mod profile;
pub mod prompt;
pub mod provider;
pub mod request;
pub mod source;
pub mod viewer;

pub use error::DocentError;
