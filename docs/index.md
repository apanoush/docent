# Docent documentation

Docent explains, translates, or answers questions about a selected passage while using the surrounding document as context. It ships as a Rust CLI and an in-repository Neovim plugin.

## Start here

- [README](../README.md): install, quickstart, and the Neovim setup snippet.
- [Configuration and profiles](config.md): config file, provider examples, profile schema, and placeholders.
- [Neovim plugin](nvim.md): commands, keymaps, float behavior, and Lua API.
- [Architecture](architecture.md): request contract, context pipeline, providers, and streaming.

## Reference

- [Rust module reference](modules/docent.md): the crate modules and their key interfaces.
- [Neovim module reference](modules/nvim.md): the Lua modules and scheduling model.

## Pipeline at a glance

1. Neovim captures the selected text, the full buffer, byte offsets, and the file name.
2. The plugin sends that data as JSON on the CLI's stdin.
3. The CLI loads `config.toml` and the requested profile, resolves effective settings, and builds the context window.
4. The CLI renders the profile prompt and calls the configured provider.
5. Text arrives either as a complete response or as newline-delimited JSON events.
6. The Neovim plugin appends streamed text to a floating window.

The CLI is usable on its own. The Neovim plugin only shells out to `docent` and renders its output.
