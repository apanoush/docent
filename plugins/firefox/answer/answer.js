/** Answer tab: receive streamed events and render the final Markdown. */

import { render } from "../lib/markdown.js";
import { parseEvent, StreamError } from "../lib/stream.js";

const statusEl = document.getElementById("status");
const warningsEl = document.getElementById("warnings");
const questionForm = document.getElementById("question-form");
const questionInput = document.getElementById("question-input");
let answerEl = document.getElementById("answer");

const sessionId = location.hash.slice(1);
const started = performance.now();

let profile = "docent";
let chunks = [];
let placeholder = true;
let finished = false;
let failed = false;
let closed = false;

function setStatus(text) {
  statusEl.textContent = text;
}

function scrollIfAtBottom(action) {
  const root = document.documentElement;
  const atBottom = window.innerHeight + window.scrollY >= root.scrollHeight - 8;
  action();
  if (atBottom) {
    window.scrollTo(0, root.scrollHeight);
  }
}

function appendText(text) {
  if (!text || finished || failed || closed) {
    return;
  }
  scrollIfAtBottom(() => {
    if (placeholder) {
      answerEl.textContent = "";
      answerEl.dataset.placeholder = "0";
      placeholder = false;
    }
    chunks.push(text);
    answerEl.appendChild(document.createTextNode(text));
  });
}

function setError(message) {
  if (closed || failed) {
    return;
  }
  failed = true;
  finished = true;
  scrollIfAtBottom(() => {
    if (placeholder) {
      answerEl.textContent = "";
      answerEl.dataset.placeholder = "0";
      placeholder = false;
    }
    const span = document.createElement("span");
    span.className = "error";
    span.textContent = `\n\nerror: ${message}`;
    answerEl.appendChild(span);
  });
  setStatus("docent: error");
  document.title = "docent: error";
}

function showAnswer(model) {
  if (closed || failed) {
    return;
  }
  finished = true;

  const body = render(chunks.join("")) || "<p>(no output)</p>";
  const block = document.createElement("div");
  block.id = "answer";
  block.className = "markdown";
  const parsed = new DOMParser().parseFromString(body, "text/html");
  block.replaceChildren(...parsed.body.childNodes);

  scrollIfAtBottom(() => {
    answerEl.replaceWith(block);
  });
  answerEl = block;
  chunks = [];

  const elapsed = ((performance.now() - started) / 1000).toFixed(1);
  const label = `docent: ${profile}${model ? ` - ${model}` : ""} (${elapsed}s)`;
  setStatus(label);
  document.title = label;
}

function showWarnings(stderr) {
  const warnings = stderr
    .split(/\r?\n/)
    .filter((line) => line.startsWith("[WARN"))
    .map((line) => {
      const separator = line.indexOf("] ");
      return separator >= 0 ? line.slice(separator + 2) : "";
    })
    .filter(Boolean);

  warningsEl.replaceChildren();
  for (const text of warnings) {
    const paragraph = document.createElement("p");
    paragraph.className = "warning";
    paragraph.textContent = `docent: ${text}`;
    warningsEl.append(paragraph);
  }
}

function handleLine(line) {
  let event;
  try {
    event = parseEvent(line);
  } catch (error) {
    if (error instanceof StreamError) {
      setError(error.message);
      return;
    }
    throw error;
  }
  if (event === null) {
    return;
  }

  if (event.type === "delta") {
    if (typeof event.text === "string") {
      appendText(event.text);
    } else {
      setError("invalid stream event");
    }
  } else if (event.type === "done") {
    showAnswer(typeof event.model === "string" ? event.model : null);
  } else if (event.type === "error") {
    setError(typeof event.message === "string" ? event.message : "unknown error");
  } else {
    setError("invalid stream event");
  }
}

function handleExit(message) {
  showWarnings(typeof message.stderr === "string" ? message.stderr : "");
  if (failed) {
    return;
  }
  if (message.code !== 0) {
    const detail = typeof message.stderr === "string" ? message.stderr.trim() : "";
    setError(detail || `docent exited with code ${message.code}`);
    return;
  }
  if (!finished) {
    showAnswer(null);
  }
}

function showQuestion() {
  answerEl.hidden = true;
  questionForm.hidden = false;
  questionInput.focus();
}

questionForm.addEventListener("submit", (event) => {
  event.preventDefault();
  const question = questionInput.value.trim();
  if (!question) {
    questionInput.focus();
    return;
  }
  questionForm.hidden = true;
  answerEl.hidden = false;
  answerEl.textContent = "Thinking...";
  setStatus(`docent: ${profile}`);
  port.postMessage({ type: "start", question });
});

questionInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter" && !event.shiftKey) {
    event.preventDefault();
    questionForm.requestSubmit();
  }
});

async function closeTab() {
  closed = true;
  window.close();
  try {
    const tab = await browser.tabs.getCurrent();
    if (tab) {
      await browser.tabs.remove(tab.id);
    }
  } catch {
    // The tab is already gone.
  }
}

document.addEventListener("keydown", (event) => {
  if (event.key !== "d" || event.ctrlKey || event.metaKey || event.altKey) {
    return;
  }
  const target = event.target;
  if (
    target instanceof HTMLElement &&
    (target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.isContentEditable)
  ) {
    return;
  }
  event.preventDefault();
  closeTab();
});

const port = browser.runtime.connect({ name: "docent-answer" });

port.onMessage.addListener((message) => {
  switch (message.type) {
    case "attached":
      profile = message.profile;
      setStatus(`docent: ${profile}`);
      break;
    case "question":
      showQuestion();
      break;
    case "line":
      handleLine(message.line);
      break;
    case "exit":
      handleExit(message);
      break;
    case "error":
      setError(message.message);
      break;
    default:
      break;
  }
});

port.onDisconnect.addListener(() => {
  if (!finished && !failed) {
    setError("lost the connection to docent");
  }
});

port.postMessage({ type: "attach", session: sessionId });

browser.runtime
  .sendMessage({ type: "css" })
  .then((response) => {
    if (response?.ok && response.css) {
      const style = document.createElement("style");
      style.textContent = response.css;
      document.head.append(style);
    }
  })
  .catch(() => {
    // Custom styling is optional.
  });
