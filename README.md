# Docent

[![CI](https://github.com/apanoush/docent/actions/workflows/ci.yml/badge.svg)](https://github.com/apanoush/docent/actions/workflows/ci.yml)
[![coverage](https://img.shields.io/badge/coverage-%E2%89%A580%25-green)](https://github.com/apanoush/docent/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/docent.svg)](https://crates.io/crates/docent)

A context-aware reading companion for the terminal, Neovim, qutebrowser, Firefox, and zathura. Select a passage, ask Docent to explain it, translate it, summarize it, or answer a question about it, and Docent answers with the surrounding document as context. 

Most lookup tools treat a word or phrase in isolation; Docent reads it in place, so "transformer" in a machine learning paper gets an architecture explanation, while "transformer" in an electrical engineering text gets something else.

## Main features

- **Context-aware prompts.** A selection is sent with its surrounding document, so the answer follows the sense the document uses.
- **Terminal commands and inputs.** `explain`, `ask`, `translate`, `summarize`, and custom profiles accept a file, stdin, or pasted text plus a line, page, or phrase selection.
- **PDF input.** Built-in extraction with page ranges and a fallback for broken font encodings, with no external tools. Scanned pages without a text layer are rejected.
- **Profiles.** Four shipped profiles plus your own TOML files, with per-profile prompts, model, temperature, token limit, context mode, and answer language. See [configuration](docs/config.md).
- **Streaming answers.** Plain text in the terminal, newline-delimited JSON events for adapters, or Server-Sent Events for a browser page.
- **Browser answer pages.** Markdown rendered once by the CLI, MathML for TeX math, and answer styles you can override. See [the browser viewer guide](docs/viewer.md).
- **Local history and cache.** Runs are recorded in SQLite, and identical rendered prompts return the stored answer without calling the provider. See [run history](docs/history.md).
- **Machine interface.** JSON on stdin for editors and browsers, plus `--print-prompt` to inspect the exact prompt and `docent extract` to see PDF text before it is sent. See [configuration](docs/config.md#machine-interface).

## Install

Requires Rust 1.88 or newer.

```bash
cargo install docent
export OPENCODE_API_KEY=...
docent init
```

Prebuilt archives and an MSI are on the [releases page](https://github.com/apanoush/docent/releases), and `cargo binstall docent` uses them when `cargo-binstall` is installed. From a clone, `cargo install --path .` builds both binaries from source.

`docent init` writes `~/.config/docent/config.toml`, four profiles (explain, ask, translate, and summarize), and an example `answer.css` for browser answer styling. The generated config points at OpenCode Go. Edit it to use OpenAI, Anthropic, another OpenAI-compatible endpoint, or a local server. The key can also be stored directly in `config.toml` with `api_key` instead of an environment variable; a set environment variable wins, and `api_key_env` remains the safer choice for shared machines and dotfiles. See [configuration](docs/config.md).

The Neovim, qutebrowser, Firefox, and zathura adapters have their own requirements and setup steps. Each section below links to its guide.

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

Summarize a whole document, or only some of its pages:

```bash
docent summarize paper.md
docent summarize paper.pdf --pages 1:4
```

Pipe a document and select a line:

```bash
cat notes.md | docent explain - --line 12
```

Stream the answer into a browser window or tab instead of the terminal:

```bash
docent explain notes.md --lines 12:20 --viewer qutebrowser
docent explain notes.md --lines 12:20 --viewer firefox --firefox-target tab
```

Answers stream by default; add `--no-stream` to wait for the complete answer. Piped stdin is always treated as the document, and a missing required input is prompted for when stdin is a terminal. When a run has no selection and the document is larger than `context.max_chars`, Docent sends only part of it and warns on stderr; use `--context-mode full`, raise the limit, or narrow the subject with `--lines` or `--pages`. The browser viewer, viewer defaults, and answer styling are documented in [the browser viewer guide](docs/viewer.md). Run `docent --help` for the terminal commands and `docent request --help` for the machine interface.

## Integrations

### Neovim

The plugin in `plugins/nvim/` captures the selection and buffer, calls the CLI asynchronously, and streams the answer into a floating window. It needs Neovim 0.10 or newer and `docent` on `PATH`; press `q` to close the float or cancel a running request. See [the Neovim guide](docs/nvim.md) for the lazy.nvim setup and options.

### qutebrowser

The adapter in `plugins/qutebrowser/` captures the selection and rendered page text, calls the CLI asynchronously, and streams the answer into a new tab. It needs qutebrowser 3.7 or newer. Closing the answer tab cancels a running request. See [the qutebrowser guide](docs/qutebrowser.md) for the `config.py` setup and limitations.

### Firefox

The adapter is a WebExtension plus a native messaging host, because WebExtensions cannot start processes. It needs Firefox 140 or newer. Install the host with `scripts/firefox-host-install.sh` on Linux or macOS and load the extension temporarily from `about:debugging`; Windows needs a manifest and registry key written by hand. See [the Firefox guide](docs/firefox.md) for the full setup, context menu, and packaging.

### Zathura

The adapter in `plugins/zathura/` reads the selection from the clipboard, extracts the current page and its neighbors through `docent extract`, and streams the answer into a terminal window, a Neovim float, or a qutebrowser page. Zathura has no keyboard text selection, so this is a mouse-first workflow. See [the zathura guide](docs/zathura.md) for the `zathurarc` binding and viewer options.

## Status

Early development, version 0.1.2. The terminal commands, streaming, profile system, Neovim plugin, qutebrowser adapter, Firefox adapter, and zathura adapter work. PDF input is text-extraction only; layout reconstruction, OCR, and password-protected PDFs are not supported. Follow-up conversations, image input, clipboard selection in the terminal commands, and a generic command provider are not implemented yet.

## Documentation

- [Documentation index](docs/index.md)
- [Configuration and profiles](docs/config.md)
- [Browser viewer](docs/viewer.md)
- [Run history](docs/history.md)
- [Neovim plugin](docs/nvim.md)
- [qutebrowser adapter](docs/qutebrowser.md)
- [Firefox adapter](docs/firefox.md)
- [Zathura adapter](docs/zathura.md)
- [Architecture](docs/architecture.md)
- [Releasing](docs/releasing.md)
- [Rust module reference](docs/modules/docent.md)
- [Neovim module reference](docs/modules/nvim.md)
- [qutebrowser module reference](docs/modules/qutebrowser.md)
- [Firefox module reference](docs/modules/firefox.md)
- [Zathura module reference](docs/modules/zathura.md)

## License

AGPL-3.0-only. See [LICENSE](LICENSE).
