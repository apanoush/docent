/** Options page: edit the settings stored in browser.storage.local. */

const form = document.getElementById("form");
const cmd = document.getElementById("cmd");
const configDir = document.getElementById("config-dir");
const defaultProfile = document.getElementById("default-profile");
const contextMode = document.getElementById("context-mode");
const targetLanguage = document.getElementById("target-language");
const openInBackground = document.getElementById("open-in-background");
const statusEl = document.getElementById("status");
const resetButton = document.getElementById("reset");
const testButton = document.getElementById("test");

const DEFAULTS = {
  cmd: "docent",
  configDir: "",
  defaultProfile: "explain",
  contextMode: "",
  targetLanguage: "",
  openInBackground: false,
};

function setStatus(text, isError = false) {
  statusEl.textContent = text;
  statusEl.classList.toggle("error", isError);
}

function fill(settings) {
  cmd.value = settings.cmd;
  configDir.value = settings.configDir;
  defaultProfile.value = settings.defaultProfile;
  contextMode.value = settings.contextMode || "";
  targetLanguage.value = settings.targetLanguage;
  openInBackground.checked = settings.openInBackground;
}

function read() {
  return {
    cmd: cmd.value.trim(),
    configDir: configDir.value.trim(),
    defaultProfile: defaultProfile.value.trim(),
    contextMode: contextMode.value,
    targetLanguage: targetLanguage.value.trim(),
    openInBackground: openInBackground.checked,
  };
}

async function load() {
  const response = await browser.runtime.sendMessage({ type: "settings" });
  if (!response?.ok) {
    setStatus(response?.error || "could not load settings", true);
    return;
  }
  fill(response.settings);
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  setStatus("Saving...");
  const response = await browser.runtime.sendMessage({
    type: "save-settings",
    settings: read(),
  });
  if (!response?.ok) {
    setStatus(response?.error || "could not save settings", true);
    return;
  }
  fill(response.settings);
  setStatus("Saved");
});

resetButton.addEventListener("click", () => {
  fill(DEFAULTS);
  setStatus("Defaults restored; press Save to apply them");
});

testButton.addEventListener("click", async () => {
  testButton.disabled = true;
  setStatus("Starting the native host...");
  try {
    const response = await browser.runtime.sendMessage({
      type: "profiles",
      force: true,
    });
    if (!response?.ok) {
      setStatus(response?.error || "could not load profiles", true);
      return;
    }
    const count = response.profiles.length;
    setStatus(`Found ${count} ${count === 1 ? "profile" : "profiles"}`);
  } finally {
    testButton.disabled = false;
  }
});

load().catch((error) => setStatus(error.message, true));
