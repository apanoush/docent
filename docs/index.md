# Docent documentation

Docent explains, translates, or answers questions about a selected passage while using the surrounding document as context. It ships as a Rust CLI with terminal commands and in-repository Neovim, qutebrowser, and zathura adapters.

## Start here

- [README](../README.md): install, quickstart, and the editor, browser, and PDF viewer setup snippets.
- [Configuration and profiles](config.md): config file, provider examples, profile schema, and placeholders.
- [Neovim plugin](nvim.md): commands, keymaps, float behavior, and Lua API.
- [qutebrowser adapter](qutebrowser.md): commands, keymaps, answer tabs, and browser limitations.
- [Zathura adapter](zathura.md): `zathurarc` bindings, clipboard capture, terminal answers, and PDF limitations.
- [Architecture](architecture.md): request contract, context pipeline, providers, and streaming.

## Reference

- [Rust module reference](modules/docent.md): the crate modules and their key interfaces.
- [Neovim module reference](modules/nvim.md): the Lua modules and scheduling model.
- [qutebrowser module reference](modules/qutebrowser.md): the Python modules, process lifecycle, and answer-tab rendering.
- [Zathura module reference](modules/zathura.md): the Python modules, two-process model, and terminal rendering.

## Pipeline at a glance

1. A terminal command reads a file or stdin and resolves a line range, phrase, or pasted text. Neovim captures the selected text, the full buffer, byte offsets, and the file name. qutebrowser captures the page selection, the rendered page text, and the page URL. Zathura reads the selection from the clipboard and extracts the current page plus its neighbors with `pdftotext`.
2. Terminal commands build the request in the CLI. The Neovim, qutebrowser, and zathura adapters send the same data as JSON on the CLI's stdin.
3. The CLI loads `config.toml` and the requested profile, resolves effective settings, and builds the context window.
4. The CLI renders the profile prompt and calls the configured provider.
5. Text arrives either as a plain stream for the terminal or as newline-delimited JSON events for the adapters.
6. The terminal prints the answer, the Neovim plugin appends text to a floating window, the qutebrowser adapter appends text to an answer tab, or the zathura adapter streams into a terminal window.

The CLI is usable on its own. The Neovim, qutebrowser, and zathura adapters only shell out to `docent` and render its output.
