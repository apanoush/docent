//! Docent library: configuration, profiles, and provider clients.
//!
//! The binary lives in `main.rs`. This crate exposes the pieces needed for
//! testing and for the JSON contract used by the Neovim plugin.

pub mod cli;
pub mod config;
pub mod context;
pub mod defaults;
pub mod error;
pub mod history;
pub mod profile;
pub mod prompt;
pub mod provider;
pub mod request;
pub mod source;

pub use error::DocentError;
