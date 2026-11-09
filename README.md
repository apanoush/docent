# Docent

A context-aware reading companion for the terminal, Neovim, qutebrowser, Firefox, and zathura. Select a passage, ask Docent to explain it, translate it, summarize it, or answer a question about it, and Docent sends the surrounding document as context to an AI provider.

Docent has five pieces:

- `docent`, a Rust CLI with terminal commands for files, line ranges, pasted text, and questions. It also accepts JSON on stdin for machine integrations, builds the prompt, calls an OpenAI-compatible or Anthropic API, and returns, streams, or shows the answer in qutebrowser or Firefox.
- `plugins/nvim/`, a Neovim plugin that captures the selection and buffer, calls the CLI asynchronously, and streams the answer into a floating window.
- `plugins/qutebrowser/`, a qutebrowser adapter that captures the page selection and rendered text, calls the CLI asynchronously, and streams the answer into a new tab.
- `plugins/firefox/`, a Firefox extension plus a native messaging host that captures the page selection and rendered text and streams the answer into a new tab.
- `plugins/zathura/`, a zathura adapter that reads the selection from the clipboard, extracts the current page with `pdftotext`, calls the CLI, and streams the answer into a terminal window, a Neovim float, or a qutebrowser window or tab.

## Why

Most lookup tools treat a word or phrase in isolation. Docent sends the selection together with the surrounding document, so "transformer" in a machine learning paper gets an architecture explanation, while "transformer" in an electrical engineering text gets something else.

## Install

Requires Rust 1.85 or newer. The Neovim plugin needs Neovim 0.10 or newer, the qutebrowser adapter needs qutebrowser 3.7 or newer, the Firefox adapter needs Firefox 140 or newer, and the zathura adapter needs zathura plus `pdftotext`. The CLI browser viewers need qutebrowser 3.7 or newer for `--viewer qutebrowser`, and Firefox with the standard `--new-tab`, `--new-window`, and `--private-window` flags for `--viewer firefox`. The zathura `--viewer nvim` option needs Neovim 0.10 or newer. The adapter tests need Node 18 or newer.

```bash
cargo install --path .
export OPENCODE_API_KEY=...
docent init
```

`docent init` writes `~/.config/docent/config.toml`, four profiles (explain, ask, translate, and summarize), and an example `answer.css` for browser answer styling. The generated config points at OpenCode Go. Edit it to use OpenAI, Anthropic, another OpenAI-compatible endpoint, or a local server. The key can also be stored directly in `config.toml` with `api_key` instead of an environment variable; a set environment variable wins, and `api_key_env` remains the safer choice for shared machines and dotfiles. See [configuration](docs/config.md).

## Quickstart

Explain lines 12 through 20 of a file:

```bash
docent explain notes.md --lines 12:20
```

Ask a question about those lines:

```bash
docent ask notes.md --lines 12:20 --question "Why does this argument work?"
```

Translate one line:

```bash
docent translate notes.md --line 12 --to French
```

Summarize a whole document, or just a section:

```bash
docent summarize paper.md
docent summarize paper.md --lines 12:40
```

Use a pasted passage with a file as context:

```bash
docent explain --text "the passage to explain" --file notes.md
```

Pipe a document and select a line:

```bash
cat notes.md | docent explain - --line 12
```

If a required input is missing and stdin is a terminal, Docent prompts for it. Piped stdin is always treated as the document. Answers stream by default; add `--no-stream` to wait for the complete answer.

Stream the answer into a browser window or tab instead of the terminal:

```bash
docent explain notes.md --lines 12:20 --viewer qutebrowser
docent explain notes.md --lines 12:20 --viewer firefox --firefox-target tab
docent summarize paper.md --viewer firefox
```

The page uses the same answer styles as the qutebrowser and Firefox adapters. Pass `--answer-css PATH` for extra overrides, or edit `<config dir>/answer.css`. See [the browser viewer guide](docs/viewer.md).

Set `[viewer] default = "firefox"` in `config.toml` to stop passing `--viewer` on every command. The other `[viewer]` keys set the executable and target, and command-line flags still win per run. `docent request` keeps `terminal` as its default unless `--viewer` is passed, so the adapters are unaffected.

`--viewer firefox` opens a standalone local answer page. It does not use the Docent Firefox extension; the extension remains the way to capture a page selection from inside Firefox.

When a run has no selection and the document is larger than `context.max_chars`, Docent sends only part of it and warns on stderr. Use `--context-mode full` to send everything, raise `context.max_chars`, or narrow the subject with `--lines`.

The JSON stdin interface used by Neovim, qutebrowser, Firefox, and zathura remains available:

```bash
printf '%s' '{"selection":"transformer","document":"A transformer maps a sequence to a sequence using attention.","byte_start":2,"byte_end":13,"file":"notes.md"}' | docent request --profile explain
```

Inspect the exact prompt without calling the provider with `--print-prompt`, or stream newline-delimited events with `--stream`. Add `--html` with `--stream` to include the final answer as rendered HTML before the `done` event; the browser adapters use this. Run `docent --help` for the terminal commands and `docent request --help` for the machine interface.

Every provider run is recorded in a local SQLite database by default, including the prompt, answer, model, parameters, token usage, and source. The database lives in the platform data directory, for example `~/.local/share/docent/docent.db` on Linux. Set `history.database` to move it, `history.enabled = false` to disable it, or pass `--no-history` for one run. See [run history](docs/history.md).

Docent also caches answers in the same database. A repeated request with the same rendered prompt, provider, model, temperature, and token limit returns the stored answer instead of calling the provider, even without an API key. Pass `--no-cache` to skip both reading and writing the cache, or `--refresh` to ignore the stored answer and replace it. Disabling history disables the cache too.

## Neovim

Add the plugin directory to your configuration with lazy.nvim:

```lua
{
  dir = "~/path/to/reading-assistant/plugins/nvim",
  config = function()
    require("docent").setup({
      keymaps = {
        explain = "<leader>de",
        ask = "<leader>da",
        translate = "<leader>dt",
        summarize = "<leader>ds",
        pick = "<leader>dp",
      },
    })
  end,
}
```

Select text and press `<leader>de`, or run `:Docent explain`. The answer streams into a floating window. Press `q` to close it; if the request is still running, `q` cancels it. See [the Neovim guide](docs/nvim.md).

## qutebrowser

Load the adapter from your qutebrowser `config.py`:

```python
import sys
sys.path.insert(0, "~/path/to/reading-assistant/plugins/qutebrowser")
import docent

docent.setup(
    config,
    keymaps={
        "explain": ",de",
        "ask": ",da",
        "translate": ",dt",
        "summarize": ",ds",
        "pick": ",dp",
    },
)
```

Select text with the mouse or in caret mode and press `,de`, or run `:docent explain`. The answer streams into a new tab. Press `d` to close the tab; closing it while the request is still running cancels the request. See [the qutebrowser guide](docs/qutebrowser.md).

## Firefox

Install the CLI and the native messaging host manifest, then load the extension temporarily:

```bash
cargo install --path .
scripts/firefox-host-install.sh
```

The script covers Linux and macOS. Windows needs a registry key and a manifest written by hand; see [the Firefox guide](docs/firefox.md#windows).

Open `about:debugging#/runtime/this-firefox`, choose **Load Temporary Add-on**, and select `plugins/firefox/manifest.json`. Select text with the mouse and click the Docent toolbar button or press `Alt+Shift+D`. The popup lists the profiles and runs the one you choose. The answer streams into a new tab. Press `d` to close the tab; closing it while the request is still running cancels the request. Permanent installs need a signed add-on from addons.mozilla.org. See [the Firefox guide](docs/firefox.md).

The extension is for capturing a page selection. To show a terminal answer in Firefox without the extension, use `--viewer firefox` as described in [the browser viewer guide](docs/viewer.md).

## Zathura

Add a binding to `~/.config/zathura/zathurarc`:

```zathurarc
map <C-e> exec "/home/you/path/to/reading-assistant/plugins/zathura/docent-zathura explain $FILE $PAGE"
```

Select text with the mouse and press `<C-e>`. The answer streams into a terminal window. Add `--viewer nvim` inside the quotes to stream into the Neovim plugin's Markdown float, or `--viewer qutebrowser` to open a qutebrowser window or tab with rendered Markdown. `ask` and `translate` work the same way; `ask` prompts for the question in the viewer. See [the zathura guide](docs/zathura.md).

## Status

Early development, version 0.1.0. The terminal commands, streaming, profile system, Neovim plugin, qutebrowser adapter, Firefox adapter, and zathura adapter work. Follow-up conversations, image input, clipboard selection in the terminal commands, and a generic command provider are not implemented yet.

## Documentation

- [Documentation index](docs/index.md)
- [Configuration and profiles](docs/config.md)
- [Neovim plugin](docs/nvim.md)
- [qutebrowser adapter](docs/qutebrowser.md)
- [Firefox adapter](docs/firefox.md)
- [Zathura adapter](docs/zathura.md)
- [Architecture](docs/architecture.md)
- [Rust module reference](docs/modules/docent.md)
- [qutebrowser module reference](docs/modules/qutebrowser.md)
- [Firefox module reference](docs/modules/firefox.md)
- [Zathura module reference](docs/modules/zathura.md)

## License

AGPL-3.0-only. See [LICENSE](LICENSE).
