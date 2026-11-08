//! Default configuration, profile, and stylesheet contents compiled into the binary.

/// Default `config.toml`, including commented provider examples.
pub const CONFIG: &str = include_str!("../defaults/config.toml");

/// Example browser answer-tab overrides created by `docent init`.
pub const ANSWER_CSS: &str = include_str!("../defaults/answer.css");

/// Default profile files, keyed by profile name.
pub const PROFILES: &[(&str, &str)] = &[
    ("explain", include_str!("../defaults/profiles/explain.toml")),
    ("ask", include_str!("../defaults/profiles/ask.toml")),
    (
        "translate",
        include_str!("../defaults/profiles/translate.toml"),
    ),
    (
        "summarize",
        include_str!("../defaults/profiles/summarize.toml"),
    ),
];
