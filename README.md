# Docent

A context-aware reading companion for the terminal and Neovim. Select a passage, ask Docent to explain it, translate it, or answer a question about it, and Docent sends the surrounding document as context to an AI provider.

Docent has two pieces:

- `docent`, a Rust CLI with terminal commands for files, line ranges, pasted text, and questions. It also accepts JSON on stdin for machine integrations, builds the prompt, calls an OpenAI-compatible or Anthropic API, and returns or streams the answer.
- `nvim/`, a Neovim plugin that captures the selection and buffer, calls the CLI asynchronously, and streams the answer into a floating window.

## Why

Most lookup tools treat a word or phrase in isolation. Docent sends the selection together with the surrounding document, so "transformer" in a machine learning paper gets an architecture explanation, while "transformer" in an electrical engineering text gets something else.

## Install

Requires Rust 1.85 or newer, and Neovim 0.10 or newer for the plugin.

```bash
cargo install --path .
export OPENCODE_API_KEY=...
docent init
```

`docent init` writes `~/.config/docent/config.toml` and three profiles: explain, ask, and translate. The generated config points at OpenCode Go. Edit it to use OpenAI, Anthropic, another OpenAI-compatible endpoint, or a local server. See [configuration](docs/config.md).

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

Use a pasted passage with a file as context:

```bash
docent explain --text "the passage to explain" --file notes.md
```

Pipe a document and select a line:

```bash
cat notes.md | docent explain - --line 12
```

If a required input is missing and stdin is a terminal, Docent prompts for it. Piped stdin is always treated as the document. Answers stream by default; add `--no-stream` to wait for the complete answer.

The JSON stdin interface used by Neovim remains available:

```bash
printf '%s' '{"selection":"transformer","document":"A transformer maps a sequence to a sequence using attention.","byte_start":2,"byte_end":13,"file":"notes.md"}' | docent run --profile explain
```

Inspect the exact prompt without calling the provider with `--print-prompt`, or stream newline-delimited events with `--stream`. Run `docent --help` for the terminal commands and `docent run --help` for the machine interface.

## Neovim

Add the plugin directory to your configuration with lazy.nvim:

```lua
{
  dir = "~/path/to/reading-assistant/nvim",
  config = function()
    require("docent").setup({
      keymaps = {
        explain = "<leader>de",
        ask = "<leader>da",
        translate = "<leader>dt",
        pick = "<leader>dp",
      },
    })
  end,
}
```

Select text and press `<leader>de`, or run `:Docent explain`. The answer streams into a floating window. Press `q` to close it; if the request is still running, `q` cancels it. See [the Neovim guide](docs/nvim.md).

## Status

Early development, version 0.1.0. The terminal commands, streaming, profile system, and Neovim plugin work. Follow-up conversations, image input, clipboard selection, and a generic command provider are not implemented yet.

## Documentation

- [Documentation index](docs/index.md)
- [Configuration and profiles](docs/config.md)
- [Neovim plugin](docs/nvim.md)
- [Architecture](docs/architecture.md)
- [Module reference](docs/modules/docent.md)

## License

GPL-3.0-only. See [LICENSE](LICENSE).
