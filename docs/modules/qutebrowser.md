# qutebrowser adapter modules

The adapter source lives in `plugins/qutebrowser/`, separate from the Rust crate. It has no Python package metadata and no third-party dependencies; qutebrowser provides the Python interpreter, Qt bindings, and command API.

## File map

| File | Responsibility |
|---|---|
| `docent/__init__.py` | Public `setup`, `refresh`, `profile_list`, and `stylesheet` entry points. Lazily imports the qutebrowser integration. |
| `docent/request.py` | Pure request builder shared by the adapter and tests. |
| `docent/stream.py` | Pure incremental NDJSON parser. |
| `docent/profiles.py` | Pure profile metadata loader around `docent profiles --json`. |
| `docent/plugin.py` | Commands, setup, JavaScript capture, `QProcess` lifecycle, and sessions. |
| `docent/markdown.py` | Pure Markdown subset renderer used by the answer tab. |
| `docent/answer.css` | Default answer-tab stylesheet; plain CSS with `--docent-*` variables, reusable by another browser front end. |
| `docent/ui.py` | Answer-tab HTML shell, stylesheet loading, and streaming DOM updates. |

`request.py`, `stream.py`, `profiles.py`, and `markdown.py` do not import qutebrowser. The unit tests exercise them without a browser.

## Loading and commands

`config.py` adds the outer `plugins/qutebrowser/` directory to `sys.path` and imports `docent`. `docent.setup(config, ...)` lazily imports `docent.plugin`, which registers the commands with `qutebrowser.api.cmdutils` and stores the settings.

Commands are registered once per process. The module checks `qutebrowser.misc.objects.commands` before registering, so a manual `importlib.reload` does not fail with a duplicate-command error. `setup` is idempotent: later calls replace settings and rebind keymaps without re-registering commands.

Keymaps are bound with qutebrowser's `ConfigAPI.bind` in both `normal` and `caret` modes. Commands have no mode restriction, so the same command is available from the command line in any mode.

## Capture

`plugin._CAPTURE_JS` returns one object from `tab.run_js_async`:

```javascript
{
  selection: "selected text",
  document: "rendered page text",
  url: "https://example.com/path?query#fragment",
  title: "Page title"
}
```

`window.getSelection().toString()` covers mouse and caret selections. `document.body.innerText` is the browser's plain-text rendering of the page. The Python side strips the query and fragment from the URL before using it as the request's `file` label.

The callback runs after the user has already left the command line, but the page selection and caret mode are still intact, so the capture does not need to freeze anything before the asynchronous call. An empty selection is rejected for profiles that require `selection`, using the same rules as the Neovim plugin.

## Process lifecycle

`Session` owns one `QProcess` and one answer tab. The process is started only after the answer tab's small HTML shell has finished loading. The request is written to stdin as UTF-8 JSON, and the write channel is closed immediately. stdout is parsed incrementally with `NdjsonParser`; stderr is buffered and shown only when the process fails without an `error` event.

The process runs with `docent [--config-dir PATH] run --profile NAME --stream [--target-language LANG] [--context-mode MODE] --source qutebrowser`. The question travels inside the JSON request rather than on the command line, so it does not appear in the process list.

`QProcess` inherits qutebrowser's environment, including the API-key variable. There is no shell: the program and arguments are passed as an array.

Closing the answer tab emits `shutting_down`, which calls `Session.cancel`. Cancellation sends `terminate()` and schedules `kill()` after one second if the process is still running. Navigating the answer tab away emits `load_started` after the initial shell load and also cancels the request.

## Answer tab

`ui.build_shell` returns a small HTML document with a sticky header, a status label, a `d close` hint, and a `<pre id="answer">` body. The `<style>` block is composed from `answer.css` first, then `:root` overrides from qutebrowser's status bar colors, then the optional user CSS from `setup(custom_css=...)`. Profile names are HTML-escaped before insertion.

`answer.css` owns the default palette and font stacks through `--docent-bg`, `--docent-fg`, `--docent-error`, `--docent-font-body`, and `--docent-font-code`. It has no qutebrowser-specific selectors, so another browser adapter can reuse it and set the same variables. `docent.stylesheet()` exports the default CSS for Python callers.

`ui.AnswerPage` injects JavaScript with `run_js_async`:

- `append_text` creates a text node in the raw answer element, accumulates the text for rendering, and auto-scrolls only when the reader is already near the bottom.
- `set_error` appends a red error line and changes the title to `docent: error`.
- `done` replaces the raw block with the rendered Markdown answer and updates the header and tab title with the profile, model, and elapsed time.
- `finish` does the same when a process exits successfully without a `done` event, showing `(no output)` if the stream was empty.
- `close` stops accepting updates after the tab is closed or the process is cancelled.

Rendering happens once, after the stream ends, so partial Markdown does not reflow as it arrives. `markdown.render` handles headings, paragraphs, lists, fenced code, blockquotes, rules, GFM tables, emphasis, strikethrough, and links. It escapes all text, keeps raw HTML and images literal, and only turns `http`, `https`, and `mailto` URLs into links, which open in a new tab. Nested lists are flattened; task lists and footnotes are not supported.

The JavaScript runs in qutebrowser's application world. It manipulates the generated page directly by element ID, so it does not depend on page scripts or globals.

## Scheduling and lifecycle edges

`run_js_async` callbacks, `QProcess` ready-read signals, and process-finished signals are delivered on Qt's main loop. UI updates are therefore safe without additional scheduling.

Sessions are stored by answer-tab ID. `docent-close` closes the current answer tab when the command runs from one, otherwise it closes the most recently opened answer tab. A finished session stays in the map until the tab closes so the close command can still find it; the `QProcess` itself is deleted when it finishes.

## Tests

`tests/qutebrowser/run.sh` runs pytest, ruff, and ty. The tests cover:

- Request field selection, offset validation, and question handling.
- NDJSON chunk boundaries, including UTF-8 characters split across reads.
- Profile metadata parsing and error paths.
- URL cleaning, request capture, answer-tab shell escaping, and streaming updates.
- Stylesheet loading and composition order, custom CSS setup, and the package export.
- Markdown block and inline rendering, tables, and escaping of raw HTML and unsafe links.
- Profile requirements, question prompting, picker dispatch, and process-failure reporting.

The qutebrowser integration is also checked end to end with `tests/qutebrowser/e2e.sh`. It starts a qutebrowser with a temporary base directory, a test `config.py`, and a fake `docent` executable that emits NDJSON. It selects page text with `jseval`, runs `:docent`, and reads the answer tab back with `jseval`. The startup sequence uses `:cmd-later`, not the deprecated `:later` alias. The script is manual and is not called by `scripts/check.sh` because it needs a display and QtWebEngine.
