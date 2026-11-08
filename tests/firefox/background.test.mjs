import assert from "node:assert/strict";
import test from "node:test";

function event() {
  const listeners = [];
  return {
    addListener: (listener) => listeners.push(listener),
    emit: (...args) => listeners.map((listener) => listener(...args)),
    listeners,
  };
}

const nativeMessages = [];
const nativePorts = [];
const createdTabs = [];
const answerMessages = [];

let capture = {
  selection: "transformer",
  document: "A transformer maps a sequence to a sequence.",
  url: "https://example.com/paper?token=1#section",
  title: "Paper",
};
let nextTabId = 10;

function createNativePort() {
  const port = {
    onMessage: event(),
    onDisconnect: event(),
    disconnected: false,
    postMessage: (message) => nativeMessages.push(message),
    disconnect: () => {
      port.disconnected = true;
      port.onDisconnect.emit();
    },
  };
  nativePorts.push(port);
  return port;
}

function createAnswerPort() {
  const port = {
    name: "docent-answer",
    onMessage: event(),
    onDisconnect: event(),
    postMessage: (message) => answerMessages.push(message),
  };
  browser.runtime.onConnect.emit(port);
  return port;
}

const browser = {
  storage: {
    local: {
      get: async (defaults) => ({ ...defaults }),
      set: async () => {},
    },
    onChanged: event(),
  },
  runtime: {
    getURL: (path) => `moz-extension://test/${path}`,
    onMessage: event(),
    onConnect: event(),
    sendNativeMessage: async (host, message) => {
      assert.equal(host, "org.docent.host");
      if (message.type === "css") {
        return { type: "result", code: 0, stdout: "", stderr: "" };
      }
      if (message.type === "exec") {
        return {
          type: "result",
          code: 0,
          stdout: JSON.stringify({
            profiles: [
              {
                name: "explain",
                description: "Explain a passage",
                requires: ["selection"],
              },
              {
                name: "ask",
                description: "Ask a question",
                requires: ["selection", "question"],
              },
            ],
          }),
          stderr: "",
        };
      }
      throw new Error(`unexpected native message: ${message.type}`);
    },
    connectNative: () => createNativePort(),
  },
  tabs: {
    query: async () => [{ id: 3 }],
    executeScript: async (tabId) => {
      assert.equal(tabId, 3);
      return [capture];
    },
    create: async ({ url, active }) => {
      const tab = { id: nextTabId++, url, active };
      createdTabs.push(tab);
      return tab;
    },
    getCurrent: async () => ({ id: 10 }),
    remove: async () => {},
    onRemoved: event(),
    onUpdated: event(),
  },
  menus: {
    removeAll: async () => {},
    create: () => {},
    onClicked: event(),
  },
  notifications: { create: async () => {} },
};

globalThis.browser = browser;
await import("../../plugins/firefox/background.js");

const onMessage = browser.runtime.onMessage.listeners[0];

function sessionId(tab) {
  return tab.url.split("#")[1];
}

test("background flow", async (t) => {
  await t.test("loads profiles through the native host", async () => {
    const response = await onMessage({ type: "profiles" });
    assert.equal(response.ok, true);
    assert.deepEqual(
      response.profiles.map((profile) => profile.name),
      ["explain", "ask"],
    );
  });

  await t.test("runs a profile and streams into the answer tab", async () => {
    const response = await onMessage({
      type: "run",
      profile: "explain",
      question: "",
      tabId: 3,
    });
    assert.deepEqual(response, { ok: true });
    assert.equal(createdTabs.length, 1);

    const tab = createdTabs[0];
    assert.match(tab.url, /answer\/answer\.html#/);
    assert.equal(tab.active, true);

    const port = createAnswerPort();
    port.onMessage.emit({ type: "attach", session: sessionId(tab) });

    assert.deepEqual(answerMessages[0], {
      type: "attached",
      profile: "explain",
    });
    assert.equal(nativePorts.length, 1);
    assert.equal(nativeMessages.length, 1);
    assert.equal(nativeMessages[0].type, "run");
    assert.deepEqual(nativeMessages[0].cmd, ["docent"]);
    assert.deepEqual(nativeMessages[0].args, [
      "run",
      "--profile",
      "explain",
      "--stream",
      "--html",
      "--source",
      "firefox",
    ]);
    assert.equal(nativeMessages[0].request.selection, "transformer");
    assert.equal(nativeMessages[0].request.file, "Paper (https://example.com/paper)");

    nativePorts[0].onMessage.emit({
      type: "line",
      line: '{"type":"delta","text":"Hello"}',
    });
    assert.deepEqual(answerMessages.at(-1), {
      type: "line",
      line: '{"type":"delta","text":"Hello"}',
    });

    nativePorts[0].onMessage.emit({
      type: "exit",
      code: 0,
      stderr: "[WARN] truncated context\n",
    });
    assert.equal(answerMessages.at(-1).type, "exit");

    port.onDisconnect.emit();
    assert.equal(nativePorts[0].disconnected, true);
  });

  await t.test("asks for a question in the answer tab", async () => {
    const response = await onMessage({
      type: "run",
      profile: "ask",
      tabId: 3,
      deferQuestion: true,
    });
    assert.deepEqual(response, { ok: true });

    const tab = createdTabs.at(-1);
    const port = createAnswerPort();
    const before = nativePorts.length;
    port.onMessage.emit({ type: "attach", session: sessionId(tab) });

    assert.equal(answerMessages.at(-1).type, "question");
    assert.equal(nativePorts.length, before);

    port.onMessage.emit({ type: "start", question: "Why does this work?" });
    assert.equal(nativePorts.length, before + 1);
    assert.equal(
      nativeMessages.at(-1).request.question,
      "Why does this work?",
    );
  });

  await t.test("rejects an empty selection before opening a tab", async () => {
    const original = capture;
    capture = { ...capture, selection: "" };
    try {
      const response = await onMessage({
        type: "run",
        profile: "explain",
        tabId: 3,
      });
      assert.deepEqual(response, { ok: false, error: "select some text first" });
      assert.equal(createdTabs.length, 2);
    } finally {
      capture = original;
    }
  });
});
