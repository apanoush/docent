/** Toolbar popup: choose a profile, optionally ask a question, and run. */

import { hasRequirement } from "../lib/profiles.js";

const form = document.getElementById("form");
const profilesBox = document.getElementById("profiles");
const questionRow = document.getElementById("question-row");
const questionInput = document.getElementById("question");
const statusEl = document.getElementById("status");
const runButton = document.getElementById("run");
const refreshButton = document.getElementById("refresh");
const optionsButton = document.getElementById("options");

let profiles = [];
let currentTabId = null;

function setStatus(text, isError = false) {
  statusEl.textContent = text;
  statusEl.classList.toggle("error", isError);
}

function selectedProfile() {
  const input = profilesBox.querySelector('input[name="profile"]:checked');
  return profiles.find((profile) => profile.name === input?.value) ?? null;
}

function updateRunState() {
  const profile = selectedProfile();
  runButton.disabled = !profile;
  const needsQuestion = profile !== null && hasRequirement(profile, "question");
  questionRow.hidden = !needsQuestion;
  if (needsQuestion) {
    questionInput.focus();
  }
}

function renderProfiles(list) {
  profiles = list;
  profilesBox.replaceChildren();
  for (const [index, profile] of list.entries()) {
    const label = document.createElement("label");
    label.className = "profile";

    const input = document.createElement("input");
    input.type = "radio";
    input.name = "profile";
    input.value = profile.name;
    input.checked = index === 0;

    const text = document.createElement("span");
    const name = document.createElement("span");
    name.className = "name";
    name.textContent = profile.name;
    text.append(name);
    if (profile.description) {
      const description = document.createElement("span");
      description.className = "description";
      description.textContent = ` - ${profile.description}`;
      text.append(description);
    }

    label.append(input, text);
    profilesBox.append(label);
  }
  updateRunState();
}

async function load(force) {
  setStatus(force ? "Refreshing profiles..." : "Loading profiles...");
  let response;
  try {
    response = await browser.runtime.sendMessage({
      type: "profiles",
      force,
    });
  } catch (error) {
    response = { ok: false, error: error.message };
  }
  if (!response?.ok) {
    profilesBox.replaceChildren();
    runButton.disabled = true;
    setStatus(response?.error || "could not load profiles", true);
    return;
  }
  if (response.profiles.length === 0) {
    profilesBox.replaceChildren();
    runButton.disabled = true;
    setStatus("no profiles found; run 'docent init'", true);
    return;
  }
  renderProfiles(response.profiles);
  setStatus("");
}

async function selectDefault() {
  const response = await browser.runtime.sendMessage({ type: "settings" });
  const name = response?.ok ? response.settings.defaultProfile : "explain";
  const input = [...profilesBox.querySelectorAll('input[name="profile"]')].find(
    (candidate) => candidate.value === name,
  );
  if (input) {
    input.checked = true;
    updateRunState();
  }
}

form.addEventListener("submit", async (event) => {
  event.preventDefault();
  const profile = selectedProfile();
  if (!profile) {
    return;
  }

  const needsQuestion = hasRequirement(profile, "question");
  const question = needsQuestion ? questionInput.value.trim() : "";
  if (needsQuestion && !question) {
    setStatus("type a question first", true);
    questionInput.focus();
    return;
  }

  runButton.disabled = true;
  refreshButton.disabled = true;
  setStatus("Running...");

  let response;
  try {
    response = await browser.runtime.sendMessage({
      type: "run",
      profile: profile.name,
      question,
      tabId: currentTabId,
    });
  } catch (error) {
    response = { ok: false, error: error.message };
  }

  if (response.ok) {
    window.close();
    return;
  }

  setStatus(response.error, true);
  runButton.disabled = false;
  refreshButton.disabled = false;
});

profilesBox.addEventListener("change", updateRunState);

refreshButton.addEventListener("click", async () => {
  refreshButton.disabled = true;
  await load(true);
  refreshButton.disabled = false;
});

optionsButton.addEventListener("click", () => {
  browser.runtime.openOptionsPage();
});

async function init() {
  const [tab] = await browser.tabs.query({ active: true, currentWindow: true });
  currentTabId = tab?.id ?? null;
  await load(false);
  if (profiles.length > 0) {
    await selectDefault();
  }
}

init().catch((error) => {
  setStatus(error.message, true);
  runButton.disabled = true;
});
