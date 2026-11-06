# Docent

A context-aware reading companion for the terminal and Neovim. Select a passage, ask Docent to explain it, translate it, or answer a question about it, and Docent sends the surrounding document as context to an AI provider.

Docent has two pieces:

- `docent`, a Rust CLI that reads a JSON request on stdin, builds the prompt, calls an OpenAI-compatible or Anthropic API, and returns or streams the answer.
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

```bash
printf '%s' '{"selection":"transformer","document":"A transformer maps a sequence to a sequence using attention.","byte_start":2,"byte_end":13,"file":"notes.md"}' | docent run --profile explain
```

Inspect the exact prompt without calling the provider:

```bash
printf '%s' '{"selection":"transformer","document":"A transformer maps a sequence to a sequence using attention.","byte_start":2,"byte_end":13}' | docent run --profile explain --print-prompt
```

Stream newline-delimited JSON events for an editor integration:

```bash
printf '%s' '{"selection":"transformer","document":"A transformer maps a sequence to a sequence using attention.","byte_start":2,"byte_end":13}' | docent run --profile explain --stream
```

Run `docent run --help` for all flags and `docent profiles --json` for profile metadata.

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

Early development, version 0.1.0. The CLI, streaming, profile system, and Neovim plugin work. Follow-up conversations, image input, and a generic command provider are not implemented yet.

## Documentation

- [Documentation index](docs/index.md)
- [Configuration and profiles](docs/config.md)
- [Neovim plugin](docs/nvim.md)
- [Architecture](docs/architecture.md)
- [Module reference](docs/modules/docent.md)

## License

GPL-3.0-only. See [LICENSE](LICENSE).
