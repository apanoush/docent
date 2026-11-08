/** Profile metadata returned by `docent profiles --json`. */

export class ProfileError extends Error {
  constructor(message) {
    super(message);
    this.name = "ProfileError";
  }
}

/** Return whether a profile declares the given input requirement. */
export function hasRequirement(profile, name) {
  return Array.isArray(profile.requires) && profile.requires.includes(name);
}

/** Parse the JSON payload printed by `docent profiles --json`. */
export function parseProfiles(payload) {
  if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
    throw new ProfileError("failed to parse profile list: expected an object");
  }
  const entries = payload.profiles;
  if (!Array.isArray(entries)) {
    throw new ProfileError(
      "failed to parse profile list: missing profiles array",
    );
  }
  return entries.map(parseProfile);
}

function parseProfile(entry) {
  if (!entry || typeof entry !== "object" || typeof entry.name !== "string") {
    throw new ProfileError("failed to parse profile list: invalid profile entry");
  }
  const name = entry.name;
  const description = entry.description;
  if (description !== undefined && description !== null && typeof description !== "string") {
    throw new ProfileError(`failed to parse profile ${name}: invalid description`);
  }
  const requires = entry.requires ?? [];
  if (!Array.isArray(requires) || !requires.every((item) => typeof item === "string")) {
    throw new ProfileError(`failed to parse profile ${name}: invalid requires`);
  }
  return {
    name,
    description: description ?? null,
    requires: [...requires],
  };
}
