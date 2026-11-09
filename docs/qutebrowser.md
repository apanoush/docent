# qutebrowser adapter

The qutebrowser adapter captures the selected text and the surrounding page, calls the `docent` CLI asynchronously, and streams the answer into a new tab. Closing the answer tab cancels a request that is still running.

## Requirements

- qutebrowser 3.7 or newer. The adapter is tested against 3.7.0 with QtWebEngine 6.11.2 and Python 3.14.
- The `docent` binary installed and available on `PATH`, or an absolute path passed through `cmd`.
- JavaScript enabled for the page. Selection capture and answer rendering use qutebrowser's JavaScript bridge.

## Install

The adapter is a Python module loaded from your qutebrowser `config.py`. It is not inside a packaged qutebrowser plugin directory, so add the outer `plugins/qutebrowser/` directory to `sys.path` and call `docent_qutebrowser.setup` with qutebrowser's `config` object:

```python
import sys
sys.path.insert(0, "~/path/to/docent/plugins/qutebrowser")
import docent_qutebrowser

docent_qutebrowser.setup(
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

`sys.path` must point at the `plugins/qutebrowser/` directory that contains the `docent_qutebrowser` package, not at the repository root. Adding the repository root would make `import qutebrowser` resolve to the adapter directory instead of the installed browser package.

Keymaps are opt-in. The adapter binds each key in both `normal` and `caret` mode so mouse selections and caret selections work. The commands themselves are available without keymaps.

If qutebrowser is launched from a desktop entry, it may not inherit the shell `PATH` or the API key environment variable. Use an absolute `cmd` path and export the key in the environment that starts qutebrowser:

```python
docent_qutebrowser.setup(
    config,
    cmd=["/home/you/.cargo/bin/docent"],
    keymaps={"explain": ",de"},
)
```

## Commands

| Command | Action |
|---|---|
| `:docent [profile]` | Run a profile against the current selection. The default profile is `explain`. |
| `:docent-pick` | Choose a profile from `docent profiles --json` and run it. |
| `:docent-close` | Close the current answer tab, or the most recent one. |
| `:docent-refresh` | Clear the cached profile list. |

`:docent` accepts the same per-request overrides as the CLI:

```text
:docent [profile] [--question TEXT] [--target-language LANG] [--context-mode auto|full|none]
```

Examples:

```text
:docent
:docent ask --question "Why does this argument work?"
:docent translate --target-language French
:docent explain --context-mode full
:docent summarize
```

The summarize profile needs no selection: with nothing selected, `:docent summarize` summarizes the whole page. Warnings logged by the CLI, such as a truncated page, appear as qutebrowser messages after a successful run.

Quote a question that contains spaces. qutebrowser's command parser splits unquoted words into separate arguments, so the command above needs the double quotes.

## What gets captured

The adapter runs one JavaScript snippet in the current tab and returns:

- `selection` from `window.getSelection().toString()`. This works for mouse selections and for selections made in caret mode. Select text with the mouse, or press `v` to enter caret mode and use the caret keys.
- `document` from `document.body.innerText`, the rendered text of the page. This is the browser equivalent of the Neovim buffer. It includes page chrome such as navigation and ads, which is accepted noise.
- `file` built from the page title and URL. Query parameters and fragments are removed before the URL reaches the model.

The first version deliberately omits byte offsets. The CLI falls back to the first exact occurrence of the selection inside the document when offsets are absent. This is exact for short pages and can pick the wrong occurrence on very long pages. Exact offsets are deferred.

## Answer tab

The answer opens in a new foreground tab by default. The tab:

- Streams text as the provider returns it.
- Shows the profile and model in the header, and updates the tab title when the stream finishes.
- Shows the answer rendered as Markdown when the stream finishes. The CLI renders it once and sends an `html` event, so every browser adapter shows the same result. TeX math in that HTML is rendered as MathML.
- Uses your status bar colors where possible.
- Can be scrolled with normal qutebrowser keys such as `j`, `k`, `gg`, `G`, and `/`.

Markdown rendering is intentionally limited. Links open in a new tab and only `http`, `https`, and `mailto` URLs become links. Raw HTML stays escaped text and images stay alt text, so model output cannot inject scripts or fetch remote resources. TeX math is rendered as MathML in the same HTML, with no JavaScript or remote assets. The Neovim float still shows raw Markdown.

## Customizing the answer style

The answer tab loads `plugins/qutebrowser/docent_qutebrowser/answer.css`. The file is plain CSS, has no qutebrowser-specific selectors, and depends only on these custom properties:

| Property | Purpose |
|---|---|
| `--docent-bg` | Page and header background |
| `--docent-fg` | Body text and derived borders |
| `--docent-error` | Error text |
| `--docent-warning` | Border of the CLI warning blocks in the zathura qutebrowser viewer |
| `--docent-font-body` | Prose, headings, lists, tables |
| `--docent-font-code` | Inline code and fenced code blocks |

Pass a CSS file to `custom_css` to override them or any other rule. `docent init` creates a commented example at `<config dir>/answer.css`; on Linux that is `~/.config/docent/answer.css`. Edit it and point `custom_css` at it:

```python
docent_qutebrowser.setup(
    config,
    custom_css="~/.config/docent/answer.css",
)
```

If the config directory is somewhere else, use the path printed by `docent init`, or run `docent config-path` and append `answer.css`.

```css
:root {
  --docent-font-body: "Source Serif 4", Georgia, serif;
  --docent-font-code: "Iosevka", monospace;
}

.markdown h2 {
  border-bottom: 1px solid color-mix(in srgb, var(--docent-fg) 20%, transparent);
}
```

The custom file is appended after the default stylesheet and after qutebrowser's palette, so its rules win. It is read during `setup`, so `:config-source` reloads CSS changes without restarting qutebrowser; Python changes still need a full restart. `docent_qutebrowser.stylesheet()` returns the default CSS, which is useful for tests or another browser adapter.

Press `d` to close the answer tab. Closing it before the request finishes terminates the `docent` process. Navigating the answer tab away from its generated page also cancels the request.

Set `open_in_background=True` in `docent_qutebrowser.setup` if you prefer the answer to open in a background tab.

## Profiles and questions

Profiles are read from `docent profiles --json` and cached for the session. Use `:docent-refresh` after adding or editing a profile.

A profile that declares `requires = ["question"]` opens a text prompt before the request starts. This is the same rule the Neovim plugin uses. A profile that requires `selection` reports "select some text first" when the page selection is empty. A profile that requires `context` reports an error when the page has no text.

## Setup options

| Option | Type | Default | Meaning |
|---|---|---|---|
| `cmd` | string or list of strings | `"docent"` | CLI executable and any fixed arguments. A list is useful for a local build or a wrapper. |
| `config_dir` | string | `None` | Passed to the CLI as `--config-dir`. Leave unset to use `DOCENT_CONFIG_DIR` or the platform config directory. |
| `default_profile` | string | `"explain"` | Profile used by `:docent` without an argument. |
| `context_mode` | string | `None` | Default `--context-mode` for every request. Values are `auto`, `full`, and `none`. |
| `target_language` | string | `None` | Default `--target-language` for every request. |
| `keymaps` | dict | `{}` | Maps `explain`, `ask`, `translate`, `summarize`, or `pick` to a key chain. Bindings are installed in normal and caret modes. |
| `open_in_background` | bool | `False` | Open answer tabs in the background. |
| `custom_css` | string | `None` | Path to a CSS file appended after the default answer styles. Read during `setup`, so `:config-source` reloads it. |

## Troubleshooting

- `docent: could not start ...`: the binary is missing from qutebrowser's `PATH`. Use `cmd` with an absolute path.
- `docent: missing API key`: neither `api_key` nor the environment variable named by `api_key_env` is set in the process that started qutebrowser.
- `docent: no profiles found`: run `docent init`, then `:docent-refresh`.
- `docent: select some text first`: select text with the mouse or enter caret mode before running the command.
- `docent: could not read this page`: the page is not ready, JavaScript is disabled, or the tab changed while the capture was running. Retry after the page loads.
- `unexpected argument '--html'`: the installed `docent` predates the shared renderer. Reinstall or update the CLI.
- No answer appears: check that the request works in a terminal with `docent request --profile explain --print-prompt`, then check the provider settings in `config.toml`.
- Changes to the adapter do not appear after `:config-source`: Python caches imported modules. Restart qutebrowser.

## Limitations

The adapter is not a first-class qutebrowser plugin. qutebrowser's extension API is not exposed to third-party extensions yet, so the integration imports internal modules and must be retested on qutebrowser upgrades.

`window.getSelection()` sees the top frame only. Iframes, shadow DOM, and PDF viewers are not supported in this version. Pages with JavaScript disabled cannot capture a selection. The whole rendered page text is sent even when the CLI will trim it, so very large pages cost encoding time and memory.

The answer is shown in a tab rather than a floating window. A float-like overlay would need to handle qutebrowser's application-wide key filter, which handles keys before widgets and binds `q` to macro recording in normal mode.
