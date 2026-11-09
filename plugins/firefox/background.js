/** Background page: capture, native messaging, and answer-tab sessions. */

import { configArgs, profilesArgs, requestArgs, contextMode } from "./lib/args.js";
import { captureTab } from "./lib/capture.js";
import { cssNative, execNative, runNative } from "./lib/native.js";
import { hasRequirement, parseProfiles } from "./lib/profiles.js";
import { buildRequest } from "./lib/request.js";

const STORAGE_DEFAULTS = Object.freeze({
  cmd: "docent",
  configDir: "",
  defaultProfile: "explain",
  contextMode: "",
  targetLanguage: "",
  openInBackground: false,
});

/** Sessions keyed by the id in the answer tab's URL fragment. */
const sessions = new Map();

let settingsCache = null;
let profilesCache = null;
let cssCache = null;

function normalizeSettings(stored) {
  const cmd = typeof stored.cmd === "string" ? stored.cmd.trim() : "";
  const defaultProfile =
    typeof stored.defaultProfile === "string" ? stored.defaultProfile.trim() : "";
  return {
    cmd: cmd || STORAGE_DEFAULTS.cmd,
    configDir:
      typeof stored.configDir === "string" ? stored.configDir.trim() : "",
    defaultProfile: defaultProfile || STORAGE_DEFAULTS.defaultProfile,
    contextMode: contextMode(stored.contextMode),
    targetLanguage:
      typeof stored.targetLanguage === "string"
        ? stored.targetLanguage.trim()
        : "",
    openInBackground: Boolean(stored.openInBackground),
  };
}

async function settings() {
  if (!settingsCache) {
    settingsCache = normalizeSettings(
      await browser.storage.local.get(STORAGE_DEFAULTS),
    );
  }
  return settingsCache;
}

async function loadProfiles(force = false) {
  const current = await settings();
  if (profilesCache && !force) {
    return profilesCache;
  }

  let result;
  try {
    result = await execNative([current.cmd], profilesArgs(current));
  } catch (error) {
    throw new Error(`could not reach the native host: ${error.message}`);
  }
  if (result?.type === "fail") {
    throw new Error(result.message);
  }
  if (result?.type !== "result") {
    throw new Error("invalid response from the native host");
  }
  if (result.code !== 0) {
    const detail = (result.stderr || "").trim();
    throw new Error(detail || `${current.cmd} exited with code ${result.code}`);
  }

  let payload;
  try {
    payload = JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(`failed to parse profile list: ${error.message}`);
  }
  profilesCache = parseProfiles(payload);
  refreshMenus(profilesCache).catch(() => {});
  return profilesCache;
}

async function loadCss() {
  if (cssCache !== null) {
    return cssCache;
  }
  const current = await settings();
  try {
    const result = await cssNative([current.cmd], configArgs(current));
    cssCache =
      result?.type === "result" && result.code === 0 ? result.stdout || "" : "";
  } catch {
    cssCache = "";
  }
  return cssCache;
}

function findProfile(profiles, name) {
  return profiles.find((profile) => profile.name === name) ?? null;
}

async function activeTabId() {
  const tabs = await browser.tabs.query({ active: true, currentWindow: true });
  return tabs.length ? tabs[0].id : null;
}

async function handleRun(message) {
  const current = await settings();
  const profiles = await loadProfiles();
  const name =
    typeof message.profile === "string" && message.profile
      ? message.profile
      : current.defaultProfile;
  const profile = findProfile(profiles, name);
  if (!profile) {
    throw new Error(`unknown profile '${name}'`);
  }

  const tabId =
    typeof message.tabId === "number" ? message.tabId : await activeTabId();
  if (tabId === null || tabId === undefined) {
    throw new Error("no active tab");
  }

  const capture = await captureTab(tabId);
  if (hasRequirement(profile, "selection") && !capture.selection.trim()) {
    throw new Error("select some text first");
  }
  if (hasRequirement(profile, "context") && !capture.document.trim()) {
    throw new Error("the page has no text to send");
  }

  const question =
    typeof message.question === "string" ? message.question.trim() : "";
  const needsQuestion = hasRequirement(profile, "question") && !question;
  if (needsQuestion && !message.deferQuestion) {
    throw new Error(`profile ${profile.name} needs a question`);
  }

  const request = buildRequest(capture, question || null);
  const args = requestArgs(current, profile.name, {
    targetLanguage: message.targetLanguage,
    contextMode: message.contextMode,
  });

  const session = {
    id: crypto.randomUUID(),
    tabId: null,
    cmd: [current.cmd],
    args,
    profile,
    request,
    port: null,
    native: null,
    started: false,
    cancelled: false,
    finished: false,
    needsQuestion,
  };
  sessions.set(session.id, session);

  const url = browser.runtime.getURL(`answer/answer.html#${session.id}`);
  let tab;
  try {
    tab = await browser.tabs.create({
      url,
      active: !current.openInBackground,
    });
  } catch (error) {
    sessions.delete(session.id);
    throw new Error(`could not open the answer tab: ${error.message}`);
  }
  session.tabId = tab.id;
  return { ok: true };
}

function startSession(session) {
  if (!session || session.started || session.cancelled) {
    return;
  }
  session.started = true;

  const port = session.port;
  if (!port) {
    return;
  }

  try {
    session.native = runNative(
      session.cmd,
      session.args,
      session.request,
      (message) => {
        if (message.type === "line") {
          port.postMessage({ type: "line", line: message.line });
        } else if (message.type === "exit") {
          session.finished = true;
          port.postMessage(message);
        } else if (message.type === "fail") {
          session.finished = true;
          port.postMessage({ type: "error", message: message.message });
        } else if (message.type === "disconnect") {
          if (!session.cancelled && !session.finished) {
            port.postMessage({
              type: "error",
              message:
                message.message || "lost the connection to docent",
            });
          }
        }
      },
    );
  } catch (error) {
    port.postMessage({
      type: "error",
      message: `could not start ${session.cmd[0]}: ${error.message}`,
    });
  }
}

function cancelSession(session) {
  if (!session || session.cancelled) {
    return;
  }
  session.cancelled = true;
  try {
    session.native?.disconnect();
  } catch {
    // The port may already be closed.
  }
  session.native = null;
  sessions.delete(session.id);
}

const MENU_ROOT = "docent";
const MENU_REFRESH = "docent-refresh";

let menusReady = Promise.resolve();

/** Rebuild the selection context menu from the current profile catalog. */
function refreshMenus(profiles) {
  if (!browser.menus) {
    return Promise.resolve();
  }
  menusReady = menusReady
    .catch(() => {
      // Recover from a previous menu failure and rebuild from scratch.
    })
    .then(async () => {
      await browser.menus.removeAll();
      browser.menus.create({
        id: MENU_ROOT,
        title: "Docent",
        contexts: ["selection"],
      });
      for (const profile of profiles) {
        browser.menus.create({
          id: `docent:${profile.name}`,
          parentId: MENU_ROOT,
          title: profile.description
            ? `${profile.name}: ${profile.description}`
            : profile.name,
          contexts: ["selection"],
        });
      }
      browser.menus.create({
        id: MENU_REFRESH,
        parentId: MENU_ROOT,
        title: "Reload profiles",
        contexts: ["selection"],
      });
    });
  return menusReady;
}

function notify(message) {
  if (!browser.notifications) {
    return;
  }
  browser.notifications
    .create({
      type: "basic",
      title: "Docent",
      message,
    })
    .catch(() => {});
}

async function runFromMenu(profileName, tab) {
  try {
    await handleRun({
      profile: profileName,
      tabId: tab?.id,
      deferQuestion: true,
    });
  } catch (error) {
    notify(error.message);
  }
}

if (browser.menus) {
  browser.menus.onClicked.addListener((info, tab) => {
    const id = String(info.menuItemId);
    if (id === MENU_REFRESH) {
      loadProfiles(true)
        .then(() => notify("Profiles reloaded"))
        .catch((error) => notify(error.message));
      return;
    }
    if (id.startsWith("docent:")) {
      runFromMenu(id.slice("docent:".length), tab);
    }
  });
}

refreshMenus([]).catch(() => {});
loadProfiles().catch(() => {});

async function handleMessage(message) {
  if (!message || typeof message !== "object") {
    return { ok: false, error: "invalid message" };
  }

  switch (message.type) {
    case "profiles":
      try {
        return { ok: true, profiles: await loadProfiles(Boolean(message.force)) };
      } catch (error) {
        return { ok: false, error: error.message };
      }
    case "settings":
      try {
        return { ok: true, settings: await settings() };
      } catch (error) {
        return { ok: false, error: error.message };
      }
    case "save-settings":
      try {
        const next = normalizeSettings({
          ...(await settings()),
          ...(message.settings || {}),
        });
        await browser.storage.local.set(next);
        settingsCache = next;
        return { ok: true, settings: next };
      } catch (error) {
        return { ok: false, error: error.message };
      }
    case "run":
      try {
        return await handleRun(message);
      } catch (error) {
        return { ok: false, error: error.message };
      }
    case "css":
      try {
        return { ok: true, css: await loadCss() };
      } catch (error) {
        return { ok: false, error: error.message };
      }
    default:
      return { ok: false, error: "unknown message" };
  }
}

browser.runtime.onMessage.addListener((message) => handleMessage(message));

browser.runtime.onConnect.addListener((port) => {
  if (port.name !== "docent-answer") {
    return;
  }

  let session = null;

  port.onMessage.addListener((message) => {
    if (message?.type === "attach") {
      session = sessions.get(message.session) ?? null;
      if (!session) {
        port.postMessage({
          type: "error",
          message: "this answer tab has no request",
        });
        return;
      }
      session.port = port;
      port.postMessage({ type: "attached", profile: session.profile.name });
      if (session.needsQuestion) {
        port.postMessage({ type: "question" });
      } else {
        startSession(session);
      }
    } else if (message?.type === "start" && session) {
      if (
        typeof message.question === "string" &&
        message.question.trim()
      ) {
        session.request.question = message.question.trim();
      }
      session.needsQuestion = false;
      startSession(session);
    } else if (message?.type === "cancel" && session) {
      cancelSession(session);
    }
  });

  port.onDisconnect.addListener(() => {
    if (session) {
      cancelSession(session);
    }
  });
});

browser.storage.onChanged.addListener(() => {
  settingsCache = null;
  profilesCache = null;
  cssCache = null;
  loadProfiles().catch(() => {});
});

browser.tabs.onRemoved.addListener((tabId) => {
  for (const session of [...sessions.values()]) {
    if (session.tabId === tabId) {
      cancelSession(session);
    }
  }
});

browser.tabs.onUpdated.addListener((tabId, changeInfo) => {
  if (!changeInfo.url) {
    return;
  }
  const answerBase = browser.runtime.getURL("answer/");
  for (const session of [...sessions.values()]) {
    if (session.tabId === tabId && !changeInfo.url.startsWith(answerBase)) {
      cancelSession(session);
    }
  }
});
