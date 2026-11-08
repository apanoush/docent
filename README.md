# Docent

A context-aware reading companion for the terminal, Neovim, qutebrowser, Firefox, and zathura. Select a passage, ask Docent to explain it, translate it, summarize it, or answer a question about it, and Docent sends the surrounding document as context to an AI provider.

Docent has five pieces:

- `docent`, a Rust CLI with terminal commands for files, line ranges, pasted text, and questions. It also accepts JSON on stdin for machine integrations, builds the prompt, calls an OpenAI-compatible or Anthropic API, and returns or streams the answer.
- `plugins/nvim/`, a Neovim plugin that captures the selection and buffer, calls the CLI asynchronously, and streams the answer into a floating window.
- `plugins/qutebrowser/`, a qutebrowser adapter that captures the page selection and rendered text, calls the CLI asynchronously, and streams the answer into a new tab.
- `plugins/firefox/`, a Firefox extension plus a native messaging host that captures the page selection and rendered text and streams the answer into a new tab.
- `plugins/zathura/`, a zathura adapter that reads the selection from the clipboard, extracts the current page with `pdftotext`, calls the CLI, and streams the answer into a terminal window.

## Why

Most lookup tools treat a word or phrase in isolation. Docent sends the selection together with the surrounding document, so "transformer" in a machine learning paper gets an architecture explanation, while "transformer" in an electrical engineering text gets something else.

## Install

Requires Rust 1.85 or newer. The Neovim plugin needs Neovim 0.10 or newer, the qutebrowser adapter needs qutebrowser 3.7 or newer, the Firefox adapter needs Firefox 140 or newer, and the zathura adapter needs zathura plus `pdftotext`. The zathura `--viewer nvim` option also needs Neovim 0.10 or newer, and `--viewer qutebrowser` needs qutebrowser 3.7 or newer. The adapter tests need Node 18 or newer.

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

When a run has no selection and the document is larger than `context.max_chars`, Docent sends only part of it and warns on stderr. Use `--context-mode full` to send everything, raise `context.max_chars`, or narrow the subject with `--lines`.

The JSON stdin interface used by Neovim, qutebrowser, Firefox, and zathura remains available:

```bash
printf '%s' '{"selection":"transformer","document":"A transformer maps a sequence to a sequence using attention.","byte_start":2,"byte_end":13,"file":"notes.md"}' | docent run --profile explain
```

Inspect the exact prompt without calling the provider with `--print-prompt`, or stream newline-delimited events with `--stream`. Run `docent --help` for the terminal commands and `docent run --help` for the machine interface.

Every provider run is recorded in a local SQLite database by default, including the prompt, answer, model, parameters, token usage, and source. The database lives in the platform data directory, for example `~/.local/share/docent/docent.db` on Linux. Set `history.database` to move it, `history.enabled = false` to disable it, or pass `--no-history` for one run. See [run history](docs/history.md).

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

Open `about:debugging#/runtime/this-firefox`, choose **Load Temporary Add-on**, and select `plugins/firefox/manifest.json`. Select text with the mouse and click the Docent toolbar button or press `Alt+Shift+D`. The popup lists the profiles and runs the one you choose. The answer streams into a new tab. Press `d` to close the tab; closing it while the request is still running cancels the request. Permanent installs need a signed add-on from addons.mozilla.org. See [the Firefox guide](docs/firefox.md).

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

GPL-3.0-only. See [LICENSE](LICENSE).
