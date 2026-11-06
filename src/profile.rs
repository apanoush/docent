//! TOML profile loading, validation, and discovery.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ContextMode;
use crate::error::DocentError;

/// Placeholder names accepted inside profile prompts.
pub const PLACEHOLDERS: &[&str] = &[
    "selection",
    "context",
    "question",
    "file",
    "response_language",
    "target_language",
];

/// Inputs a profile requires before it can be rendered.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Requirement {
    /// A non-empty selected passage.
    Selection,
    /// A non-empty follow-up question.
    Question,
    /// A non-empty surrounding document.
    Context,
}

impl Requirement {
    /// Name used in TOML, JSON, and placeholder syntax.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selection => "selection",
            Self::Question => "question",
            Self::Context => "context",
        }
    }
}

/// A single profile file.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    /// Short description shown by the profile picker.
    pub description: Option<String>,
    /// Model override for this profile.
    pub model: Option<String>,
    /// Temperature override for this profile.
    pub temperature: Option<f32>,
    /// Output token limit override for this profile.
    pub max_tokens: Option<u32>,
    /// Context mode override for this profile.
    pub context_mode: Option<ContextMode>,
    /// Response language override for this profile.
    pub response_language: Option<String>,
    /// Translation target language override for this profile.
    pub target_language: Option<String>,
    /// Inputs required before invoking this profile.
    #[serde(default)]
    pub requires: Vec<Requirement>,
    /// Optional profile-specific system message.
    pub system_prompt: Option<String>,
    /// User message template.
    pub user_prompt: String,
}

impl Profile {
    /// Checks values that TOML parsing cannot enforce.
    pub fn validate(&self, path: &Path) -> Result<(), DocentError> {
        let mut errors = Vec::new();

        if self.user_prompt.trim().is_empty() {
            errors.push("user_prompt must not be empty".to_string());
        }
        if let Some(description) = &self.description {
            if description.trim().is_empty() {
                errors.push("description must not be empty".to_string());
            }
        }
        if let Some(model) = &self.model {
            if model.trim().is_empty() {
                errors.push("model must not be empty".to_string());
            }
        }
        if let Some(temperature) = self.temperature {
            if !(0.0..=2.0).contains(&temperature) {
                errors.push(format!(
                    "temperature must be between 0.0 and 2.0, got {temperature}"
                ));
            }
        }
        if let Some(max_tokens) = self.max_tokens {
            if max_tokens == 0 {
                errors.push("max_tokens must be greater than zero".to_string());
            }
        }
        if let Some(language) = &self.response_language {
            if language.trim().is_empty() {
                errors.push("response_language must not be empty".to_string());
            }
        }
        if let Some(language) = &self.target_language {
            if language.trim().is_empty() {
                errors.push("target_language must not be empty".to_string());
            }
        }
        if self.context_mode == Some(ContextMode::None)
            && self.requires.contains(&Requirement::Context)
        {
            errors.push("requires includes 'context' but context_mode is 'none'".to_string());
        }

        let mut seen = HashSet::new();
        for requirement in &self.requires {
            if !seen.insert(*requirement) {
                errors.push(format!("duplicate requirement '{}'", requirement.as_str()));
            }
        }

        let user_placeholders = match template_placeholders(&self.user_prompt) {
            Ok(names) => names,
            Err(message) => {
                errors.push(format!("user_prompt: {message}"));
                Vec::new()
            }
        };
        let system_placeholders = match &self.system_prompt {
            Some(system) => match template_placeholders(system) {
                Ok(names) => names,
                Err(message) => {
                    errors.push(format!("system_prompt: {message}"));
                    Vec::new()
                }
            },
            None => Vec::new(),
        };

        for name in user_placeholders.iter().chain(system_placeholders.iter()) {
            if !PLACEHOLDERS.contains(&name.as_str()) {
                errors.push(format!("unknown placeholder '{{{{{name}}}}}'"));
            }
        }

        let used: HashSet<&str> = user_placeholders
            .iter()
            .chain(system_placeholders.iter())
            .map(String::as_str)
            .collect();

        for requirement in &self.requires {
            let name = requirement.as_str();
            if !used.contains(name) {
                errors.push(format!(
                    "requires '{name}' but no prompt uses '{{{{{name}}}}}'"
                ));
            }
        }
        for implicit in ["selection", "question"] {
            if used.contains(implicit)
                && !self
                    .requires
                    .iter()
                    .any(|requirement| requirement.as_str() == implicit)
            {
                errors.push(format!(
                    "prompt uses '{{{{{implicit}}}}}' but requires does not include '{implicit}'"
                ));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(DocentError::InvalidProfile {
                path: path.to_path_buf(),
                message: errors.join("; "),
            })
        }
    }
}

/// Extracts placeholder names from a prompt template.
///
/// Placeholders use `{{name}}`; whitespace inside the braces is allowed.
/// Names are returned once, in first-appearance order.
pub fn template_placeholders(template: &str) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            return Err("unclosed placeholder".to_string());
        };
        let name = after_start[..end].trim();
        if name.is_empty() {
            return Err("empty placeholder".to_string());
        }
        if !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        {
            return Err(format!("invalid placeholder name '{name}'"));
        }
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_string());
        }
        rest = &after_start[end + 2..];
    }

    Ok(names)
}

/// Metadata used by `docent profiles` and the Neovim picker.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProfileInfo {
    /// Profile name, equal to the file stem.
    pub name: String,
    /// Optional short description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Inputs required before invoking the profile.
    pub requires: Vec<Requirement>,
}

/// Profiles found on disk, plus warnings for files that were skipped.
#[derive(Debug, Default)]
pub struct ProfileCatalog {
    /// Valid profiles, sorted by name.
    pub profiles: Vec<ProfileInfo>,
    /// Validation failures for profiles that were skipped.
    pub warnings: Vec<String>,
}

/// Loads profiles from a directory.
#[derive(Debug, Clone)]
pub struct ProfileStore {
    dir: PathBuf,
}

impl ProfileStore {
    /// Creates a store rooted at `dir`.
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Directory containing profile files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Loads and validates one profile by name.
    pub fn load(&self, name: &str) -> Result<Profile, DocentError> {
        if !is_valid_profile_name(name) {
            return Err(DocentError::InvalidProfileName(name.to_string()));
        }
        let path = self.dir.join(format!("{name}.toml"));
        if !path.exists() {
            return Err(DocentError::ProfileNotFound {
                name: name.to_string(),
                dir: self.dir.clone(),
            });
        }
        load_profile(&path)
    }

    /// Lists valid profiles and collects warnings for invalid files.
    pub fn discover(&self) -> Result<ProfileCatalog, DocentError> {
        let entries = fs::read_dir(&self.dir).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                DocentError::NoProfiles(self.dir.clone())
            } else {
                DocentError::ReadDir {
                    path: self.dir.clone(),
                    source,
                }
            }
        })?;

        let mut catalog = ProfileCatalog::default();
        for entry in entries {
            let entry = entry.map_err(|source| DocentError::ReadDir {
                path: self.dir.clone(),
                source,
            })?;
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                catalog
                    .warnings
                    .push(format!("skipping {}: invalid filename", path.display()));
                continue;
            };
            if !is_valid_profile_name(name) {
                catalog.warnings.push(format!(
                    "skipping {}: invalid profile name '{name}'",
                    path.display()
                ));
                continue;
            }
            match load_profile(&path) {
                Ok(profile) => catalog.profiles.push(ProfileInfo {
                    name: name.to_string(),
                    description: profile.description,
                    requires: profile.requires,
                }),
                Err(error) => catalog.warnings.push(error.to_string()),
            }
        }

        catalog
            .profiles
            .sort_by(|left, right| left.name.cmp(&right.name));

        if catalog.profiles.is_empty() {
            if catalog.warnings.is_empty() {
                return Err(DocentError::NoProfiles(self.dir.clone()));
            }
            return Err(DocentError::NoValidProfiles {
                dir: self.dir.clone(),
                messages: catalog.warnings.join("; "),
            });
        }

        Ok(catalog)
    }
}

/// Returns true for profile names that are safe to use as file stems.
pub fn is_valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '-'
        })
}

/// Reads, parses, and validates one profile file.
pub fn load_profile(path: &Path) -> Result<Profile, DocentError> {
    let contents = fs::read_to_string(path).map_err(|source| DocentError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let profile: Profile =
        toml::from_str(&contents).map_err(|source| DocentError::ParseProfile {
            path: path.to_path_buf(),
            source,
        })?;
    profile.validate(path)?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defaults;

    fn profile(user_prompt: &str) -> Profile {
        Profile {
            description: None,
            model: None,
            temperature: None,
            max_tokens: None,
            context_mode: None,
            response_language: None,
            target_language: None,
            requires: Vec::new(),
            system_prompt: None,
            user_prompt: user_prompt.to_string(),
        }
    }

    #[test]
    fn default_profiles_are_valid() {
        for (name, contents) in defaults::PROFILES {
            let parsed: Profile = toml::from_str(contents).unwrap();
            parsed.validate(Path::new(&format!("{name}.toml"))).unwrap();
        }
    }

    #[test]
    fn unknown_profile_fields_are_rejected() {
        let error = toml::from_str::<Profile>("user_prompt = \"hi\"\nextra = true\n").unwrap_err();
        assert!(error.to_string().contains("extra"));
    }

    #[test]
    fn missing_user_prompt_is_rejected() {
        let error = toml::from_str::<Profile>("description = \"x\"\n").unwrap_err();
        assert!(error.to_string().contains("user_prompt"));
    }

    #[test]
    fn placeholder_parser_handles_whitespace_and_duplicates() {
        let names =
            template_placeholders("a {{selection}} b {{ question }} c {{selection}}").unwrap();
        assert_eq!(names, vec!["selection", "question"]);
    }

    #[test]
    fn placeholder_parser_rejects_bad_syntax() {
        assert!(template_placeholders("{{}}").is_err());
        assert!(template_placeholders("{{a").is_err());
        assert!(template_placeholders("{{a b}}").is_err());
    }

    #[test]
    fn valid_profile_passes() {
        let mut parsed = profile("{{selection}}");
        parsed.requires = vec![Requirement::Selection];
        parsed.validate(Path::new("x.toml")).unwrap();
    }

    #[test]
    fn unknown_placeholder_is_rejected() {
        let parsed = profile("{{unknown}}");
        let error = parsed.validate(Path::new("x.toml")).unwrap_err();
        assert!(error.to_string().contains("unknown placeholder"));
    }

    #[test]
    fn question_placeholder_requires_declaration() {
        let parsed = profile("{{question}}");
        let error = parsed.validate(Path::new("x.toml")).unwrap_err();
        assert!(error.to_string().contains("requires"));
    }

    #[test]
    fn unused_requirement_is_rejected() {
        let mut parsed = profile("hello");
        parsed.requires = vec![Requirement::Selection];
        let error = parsed.validate(Path::new("x.toml")).unwrap_err();
        assert!(error.to_string().contains("no prompt uses"));
    }

    #[test]
    fn duplicate_requirement_is_rejected() {
        let mut parsed = profile("{{selection}}");
        parsed.requires = vec![Requirement::Selection, Requirement::Selection];
        let error = parsed.validate(Path::new("x.toml")).unwrap_err();
        assert!(error.to_string().contains("duplicate"));
    }

    #[test]
    fn context_requirement_conflicts_with_none_mode() {
        let mut parsed = profile("{{context}}");
        parsed.requires = vec![Requirement::Context];
        parsed.context_mode = Some(ContextMode::None);
        let error = parsed.validate(Path::new("x.toml")).unwrap_err();
        assert!(error.to_string().contains("context_mode"));
    }

    #[test]
    fn invalid_numbers_are_rejected() {
        let mut parsed = profile("hi");
        parsed.temperature = Some(2.5);
        parsed.max_tokens = Some(0);
        let error = parsed.validate(Path::new("x.toml")).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("temperature"));
        assert!(message.contains("max_tokens"));
    }

    #[test]
    fn profile_name_validation_blocks_traversal() {
        assert!(is_valid_profile_name("explain"));
        assert!(is_valid_profile_name("my-profile_2"));
        assert!(!is_valid_profile_name(""));
        assert!(!is_valid_profile_name("../secret"));
        assert!(!is_valid_profile_name("has space"));
    }

    #[test]
    fn load_rejects_invalid_name_and_missing_profile() {
        let temp = tempfile::tempdir().unwrap();
        let store = ProfileStore::new(temp.path().to_path_buf());

        let error = store.load("../secret").unwrap_err();
        assert!(matches!(error, DocentError::InvalidProfileName(_)));

        let error = store.load("missing").unwrap_err();
        assert!(matches!(error, DocentError::ProfileNotFound { .. }));
    }

    #[test]
    fn discover_skips_invalid_files_and_sorts() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("profiles");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("zeta.toml"), "user_prompt = \"hello\"\n").unwrap();
        fs::write(dir.join("alpha.toml"), "user_prompt = \"hello\"\n").unwrap();
        fs::write(dir.join("bad.toml"), "user_prompt = \"{{question}}\"\n").unwrap();
        fs::write(dir.join("notes.txt"), "ignored").unwrap();
        fs::write(dir.join("bad name.toml"), "user_prompt = \"hello\"\n").unwrap();

        let catalog = ProfileStore::new(dir).discover().unwrap();
        let names: Vec<_> = catalog
            .profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
        assert_eq!(catalog.warnings.len(), 2);
    }

    #[test]
    fn discover_errors_when_nothing_is_valid() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("profiles");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("bad.toml"), "user_prompt = \"{{question}}\"\n").unwrap();

        let error = ProfileStore::new(dir).discover().unwrap_err();
        assert!(matches!(error, DocentError::NoValidProfiles { .. }));
    }

    #[test]
    fn discover_errors_on_empty_directory() {
        let temp = tempfile::tempdir().unwrap();
        let error = ProfileStore::new(temp.path().to_path_buf())
            .discover()
            .unwrap_err();
        assert!(matches!(error, DocentError::NoProfiles(_)));
    }

    #[test]
    fn discover_reports_missing_directory_as_no_profiles() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let error = ProfileStore::new(missing).discover().unwrap_err();
        assert!(matches!(error, DocentError::NoProfiles(_)));
    }
}
