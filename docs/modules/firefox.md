# Firefox adapter modules

The adapter has two halves. `plugins/firefox/` is a WebExtension with no build step and no third-party JavaScript. `src/native_host.rs` and `src/bin/docent-native-host.rs` are the Rust bridge that lets the extension run the CLI.

## File map

| File | Responsibility |
|---|---|
| `plugins/firefox/manifest.json` | Manifest V2, permissions, add-on ID, toolbar button, shortcut, options page. |
| `plugins/firefox/background.html` | Persistent background page that loads `background.js` as a module. |
| `plugins/firefox/background.js` | Settings, profile cache, native sessions, context menus, notifications. |
| `plugins/firefox/lib/native.js` | `connectNative` and `sendNativeMessage` wrappers for the host. |
| `plugins/firefox/lib/capture.js` | `tabs.executeScript` snippet that returns the selection and page text. |
| `plugins/firefox/lib/request.js` | Request builder, URL cleaning, file label. |
| `plugins/firefox/lib/stream.js` | NDJSON line parser and stream error. |
| `plugins/firefox/lib/profiles.js` | Profile metadata parser and requirement checks. |
| `plugins/firefox/lib/args.js` | `docent` argv construction. |
| `plugins/firefox/lib/markdown.js` | Markdown subset renderer, ported from the qutebrowser renderer. |
| `plugins/firefox/popup/` | Profile picker and question input. |
| `plugins/firefox/answer/` | Answer tab, question form, bundled and user styles. |
| `plugins/firefox/options/` | Settings page. |
| `scripts/firefox-host-install.sh` | Writes the native messaging host manifest. |
| `scripts/firefox-host-uninstall.sh` | Removes it. |
| `src/native_host.rs` | Framing, request parsing, spawn, forwarding, `exec`, `css`. |
| `src/bin/docent-native-host.rs` | Thin entry point for the host binary. |

`lib/stream.js`, `lib/request.js`, `lib/profiles.js`, `lib/args.js`, and `lib/markdown.js` do not touch the `browser` API. The Node tests exercise them without Firefox.

## Loading and permissions

The manifest uses Manifest V2 with `background.page` so the background script can be an ES module and stay alive for the whole browser session. `activeTab` grants access to the current tab after the user clicks the toolbar button, uses the shortcut, or picks a context-menu item. It is not a host permission, so Firefox shows no install-time warning for it. `tabs.executeScript` works in MV2 once `activeTab` is granted.

## Capture

`capture.js` injects one IIFE with `tabs.executeScript`. The IIFE returns `selection`, `document`, `url`, and `title`. The background drops `url` and `title` after building the `file` label. Query parameters, fragments, and credentials are removed with `URL`.

Offsets are not captured. `window.getSelection()` and `document.body.innerText` do not share a coordinate space, so a guessed UTF-8 byte offset would often be wrong.

## Sessions and native process

A run creates a session keyed by a random UUID. The UUID travels in the answer tab's URL fragment. The answer page connects with `runtime.connect({name: "docent-answer"})` and sends `attach`; the background finds the session, sends `attached` with the profile name, and starts the native port unless the profile still needs a question.

`runNative` opens `runtime.connectNative("org.docent.host")` and posts one `run` message. The background forwards each `line` message to the answer page, then the `exit` message with the code and captured stderr. A `fail` message becomes an `error` message. If the port disconnects before the process exits, the answer page shows a connection error.

Closing the answer tab, navigating it away, or closing the browser disconnects the port. Firefox sends `SIGTERM` to the host and its child, then `SIGKILL` after a grace period. The background also deletes the session on `tabs.onRemoved` and on a URL change away from the answer page.

One host process handles one request. The profile catalog and custom stylesheet use `runtime.sendNativeMessage`, which starts a short-lived host for each call.

## Native protocol

Messages are a 4-byte native-endian length prefix followed by UTF-8 JSON.

The extension sends one of:

```json
{"type":"run","cmd":["docent"],"args":["run","--profile","explain","--stream"],"request":{"selection":"...","document":"..."}}
{"type":"exec","cmd":["docent"],"args":["profiles","--json"]}
{"type":"css","cmd":["docent"],"args":[]}
```

The host replies:

```json
{"type":"line","line":"{\"type\":\"delta\",\"text\":\"...\"}"}
{"type":"exit","code":0,"stderr":""}
{"type":"result","code":0,"stdout":"...","stderr":"..."}
{"type":"fail","message":"..."}
```

`run` forwards complete stdout lines and then one `exit`. `exec` runs to completion and returns clipped stdout and stderr. `css` runs `docent config-path` and reads `answer.css` from that directory; a missing file is an empty result, not an error.

The host refuses to start a program whose basename does not begin with `docent`. The add-on ID in the host manifest's `allowed_extensions` is the main trust boundary.

## Answer page

The answer page accumulates `delta` text in a `<pre>` and keeps the reader pinned to the bottom only when they are already there. At `done`, or at a zero exit without `done`, it renders the accumulated text with the Markdown subset renderer and replaces the `<pre>` with a `<div class="markdown">`. The header and tab title show the profile, model, and elapsed time. A non-zero exit or an `error` event adds a red error line. `[WARN` lines from stderr are shown above the answer.

The renderer escapes all text, keeps raw HTML and images literal, and only turns `http`, `https`, and `mailto` URLs into links. Links open in a new tab.

Press `d` to close the tab unless focus is in an input. `window.close()` closes an extension tab; `tabs.remove` is the fallback.

The bundled `answer/answer.css` is a copy of `plugins/qutebrowser/docent/answer.css`, and `tests/firefox/run.sh` fails when the two files differ. `answer/page.css` holds Firefox-only components. When `<config dir>/answer.css` exists, the answer page appends it as a `<style>` element after the bundled stylesheet.

## Profiles, questions, and menus

`profiles --json` is parsed once per session and cached. A refresh clears the cache and rebuilds the context menu. The menu has a `Docent` parent with one item per profile and a `Reload profiles` item.

The popup shows the question field only for profiles that declare `question`. A context-menu run passes `deferQuestion` instead, and the answer page shows the form before starting the host. Selection and context requirements are checked before the answer tab opens, with the same messages as the qutebrowser adapter.

## Options

The options page stores the command, config directory, default profile, context mode, target language, and background-tab preference in `browser.storage.local`. The background normalizes the values and clears its settings, profile, and stylesheet caches when storage changes. The **Test** button forces a profile load so a missing host or binary is reported before the first real run.

## Tests

`tests/firefox/run.sh` runs the Node test runner, checks that the two answer stylesheets match, validates the manifest fields the host depends on, and runs `web-ext lint` when it is installed.

The Node tests cover NDJSON parsing (including a surrogate pair split across chunks), the Markdown renderer and its escaping rules, request building, URL cleaning, profile parsing, and argv construction. `background.test.mjs` mocks the `browser` API and exercises the full background flow: profile loading, capture, session creation, attach, streaming, exit, question deferral, and cancellation. `tests/firefox/e2e.sh` is a manual harness that starts Firefox through `web-ext run` with a fake `docent` and a temporary native host manifest.

The Rust side is tested in `src/native_host.rs` with a fake `docent` shell script: framing, partial headers, oversized messages, invalid requests, spawn failures, line forwarding, exit codes, `exec`, and `css`.
