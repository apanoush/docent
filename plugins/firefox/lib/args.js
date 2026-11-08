/** Build the argv passed to `docent`. */

const CONTEXT_MODES = new Set(["", "auto", "full", "none"]);

/** Return a validated context mode, or an empty string for the default. */
export function contextMode(value) {
  return CONTEXT_MODES.has(value) ? value : "";
}

/** Arguments shared by every command that talks to the CLI. */
export function configArgs(settings) {
  const result = [];
  if (settings.configDir) {
    result.push("--config-dir", settings.configDir);
  }
  return result;
}

/** Arguments for `docent profiles --json`. */
export function profilesArgs(settings) {
  return configArgs(settings).concat(["profiles", "--json"]);
}

/** Arguments for `docent run`. */
export function runArgs(settings, profile, overrides = {}) {
  const args = ["run", "--profile", profile, "--stream"];
  const targetLanguage = overrides.targetLanguage ?? settings.targetLanguage;
  const mode = contextMode(overrides.contextMode ?? settings.contextMode);
  if (targetLanguage) {
    args.push("--target-language", targetLanguage);
  }
  if (mode) {
    args.push("--context-mode", mode);
  }
  args.push("--source", "firefox");
  return configArgs(settings).concat(args);
}
