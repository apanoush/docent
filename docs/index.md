# Docent documentation

Docent explains, translates, summarizes, or answers questions about a selected passage while using the surrounding document as context. It ships as a Rust CLI with terminal commands and in-repository Neovim, qutebrowser, Firefox, and zathura adapters.

## Start here

- [README](../README.md): install, quickstart, and pointers to the editor, browser, and PDF viewer guides.
- [Configuration and profiles](config.md): config file, provider examples, profile schema, and placeholders.
- [Browser viewer](viewer.md): stream a terminal answer into a qutebrowser or Firefox window or tab, and style it.
- [Run history](history.md): SQLite location, schema, source tagging, privacy, and example queries.
- [Neovim plugin](nvim.md): commands, keymaps, float behavior, and Lua API.
- [qutebrowser adapter](qutebrowser.md): commands, keymaps, answer tabs, and browser limitations.
- [Firefox adapter](firefox.md): native messaging host, toolbar picker, context menu, answer tabs, and limitations.
- [Zathura adapter](zathura.md): `zathurarc` bindings, clipboard capture, terminal, Neovim, or qutebrowser answers, and PDF limitations.
- [Architecture](architecture.md): request contract, context pipeline, providers, and streaming.

## Reference

- [Rust module reference](modules/docent.md): the crate modules and their key interfaces.
- [Neovim module reference](modules/nvim.md): the Lua modules and scheduling model.
- [qutebrowser module reference](modules/qutebrowser.md): the Python modules, process lifecycle, and answer-tab rendering.
- [Firefox module reference](modules/firefox.md): the WebExtension modules, native protocol, and session lifecycle.
- [Zathura module reference](modules/zathura.md): the Python modules, two-process model, and terminal rendering.

## Pipeline at a glance

1. A terminal command reads a file or stdin and resolves a line range, page range, phrase, or pasted text. PDFs are decoded to text first. Neovim captures the selected text, the full buffer, byte offsets, and the file name. qutebrowser captures the page selection, the rendered page text, and the page URL. Firefox captures the same fields through a WebExtension and sends them through a native messaging host. Zathura reads the selection from the clipboard and extracts the current page plus its neighbors with `docent extract --pages`.
2. Terminal commands build the request in the CLI. The Neovim, qutebrowser, and zathura adapters send the same data as JSON on the CLI's stdin. The Firefox host writes the same JSON to the CLI's stdin.
3. The CLI loads `config.toml` and the requested profile, resolves effective settings, and builds the context window.
4. The CLI renders the profile prompt and consults the local answer cache. An identical rendered prompt with the same provider, model, temperature, and token limit reuses the stored answer; otherwise Docent calls the configured provider and stores the result.
5. Text arrives either as a plain stream for the terminal, as newline-delimited JSON events for the adapters, or as Server-Sent Events for the CLI browser viewer.
6. The terminal prints the answer, the Neovim plugin appends text to a floating window, the qutebrowser adapter appends text to an answer tab, the Firefox adapter streams into an extension tab, the zathura adapter streams into a terminal window, the Neovim float, or a qutebrowser page, or the CLI streams into its own qutebrowser page. Browser front ends consume a final `html` event rendered once by the CLI, including MathML for TeX math.
7. The CLI records the run in a local SQLite database when history is enabled. Failures to write history are warnings, not request failures.

The CLI is usable on its own. The Neovim, qutebrowser, Firefox, and zathura adapters only shell out to `docent` and render its output. The Firefox adapter reaches the CLI through a small native messaging host because WebExtensions cannot start processes.
