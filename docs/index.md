# Docent documentation

Docent explains, translates, or answers questions about a selected passage while using the surrounding document as context. It ships as a Rust CLI with terminal commands, an in-repository Neovim plugin, and a qutebrowser adapter.

## Start here

- [README](../README.md): install, quickstart, and the editor and browser setup snippets.
- [Configuration and profiles](config.md): config file, provider examples, profile schema, and placeholders.
- [Neovim plugin](nvim.md): commands, keymaps, float behavior, and Lua API.
- [qutebrowser adapter](qutebrowser.md): commands, keymaps, answer tabs, and browser limitations.
- [Architecture](architecture.md): request contract, context pipeline, providers, and streaming.

## Reference

- [Rust module reference](modules/docent.md): the crate modules and their key interfaces.
- [Neovim module reference](modules/nvim.md): the Lua modules and scheduling model.
- [qutebrowser module reference](modules/qutebrowser.md): the Python modules, process lifecycle, and answer-tab rendering.

## Pipeline at a glance

1. A terminal command reads a file or stdin and resolves a line range, phrase, or pasted text. Neovim captures the selected text, the full buffer, byte offsets, and the file name. qutebrowser captures the page selection, the rendered page text, and the page URL.
2. Terminal commands build the request in the CLI. The Neovim and qutebrowser adapters send the same data as JSON on the CLI's stdin.
3. The CLI loads `config.toml` and the requested profile, resolves effective settings, and builds the context window.
4. The CLI renders the profile prompt and calls the configured provider.
5. Text arrives either as a plain stream for the terminal or as newline-delimited JSON events for the adapters.
6. The terminal prints the answer, the Neovim plugin appends text to a floating window, or the qutebrowser adapter appends text to an answer tab.

The CLI is usable on its own. The Neovim and qutebrowser adapters only shell out to `docent` and render its output.
