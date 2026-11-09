# Firefox adapter

The Firefox adapter captures the selected text and the surrounding page, calls the `docent` CLI through a native messaging host, and streams the answer into a new tab. Closing the answer tab cancels a request that is still running.

## Requirements

- Firefox 140 or newer. Firefox 140 introduced the data collection permissions key that the manifest declares.
- The `docent` and `docent-native-host` binaries. `cargo install --path .` installs both.
- The native messaging host manifest installed with `scripts/firefox-host-install.sh` on Linux or macOS, or by hand on Windows.
- Node 18 or newer for the adapter tests and for packaging with `web-ext`.

## Install

Install the CLI and the host manifest:

```bash
cargo install --path .
scripts/firefox-host-install.sh
```

The script writes the manifest where Firefox looks for the platform:

| Platform | Manifest location |
|---|---|
| Linux | `~/.mozilla/native-messaging-hosts/org.docent.host.json` |
| macOS | `~/Library/Application Support/Mozilla/NativeMessagingHosts/org.docent.host.json` |
| Windows | `%APPDATA%\Mozilla\NativeMessagingHosts\org.docent.host.json`, registered under `HKCU\SOFTWARE\Mozilla\NativeMessagingHosts\org.docent.host` |

The manifest holds the absolute path to `docent-native-host` and the add-on ID `{ae094259-bf49-44a7-a0f0-186c52340b02}`. Pass `--host PATH` if the binary is elsewhere, or `--force` to replace an existing manifest. `--manifest-dir DIR` and `DOCENT_FIREFOX_MANIFEST_DIR` override the platform default, which Snap and Flatpak builds need. The script covers Linux and macOS and exits with a pointer to the Windows steps below.

### Windows

Firefox finds the host through a registry key, so install the manifest by hand. From PowerShell:

```powershell
$host = (Get-Command docent-native-host).Source
$manifest = "$env:APPDATA\Mozilla\NativeMessagingHosts\org.docent.host.json"
New-Item -ItemType Directory -Force -Path (Split-Path $manifest) | Out-Null
$json = @{
  name = "org.docent.host"
  description = "Native messaging host for the Docent Firefox adapter"
  path = $host
  type = "stdio"
  allowed_extensions = @("{ae094259-bf49-44a7-a0f0-186c52340b02}")
} | ConvertTo-Json
[IO.File]::WriteAllText($manifest, $json)
$key = "HKCU:\SOFTWARE\Mozilla\NativeMessagingHosts\org.docent.host"
New-Item -Path $key -Force | Out-Null
Set-Item -Path $key -Value $manifest
```

To remove the host, delete the key and the manifest:

```powershell
Remove-Item -Recurse "HKCU:\SOFTWARE\Mozilla\NativeMessagingHosts\org.docent.host"
Remove-Item "$env:APPDATA\Mozilla\NativeMessagingHosts\org.docent.host.json"
```

Load the extension for development:

1. Open `about:debugging#/runtime/this-firefox`.
2. Choose **Load Temporary Add-on**.
3. Select `plugins/firefox/manifest.json`.

A temporary add-on disappears when Firefox restarts. Release and beta Firefox only install signed add-ons, so a permanent install needs a signed XPI. See [Permanent install and sharing](#permanent-install-and-sharing).

If Firefox is a Snap or Flatpak build, its native messaging directory is inside the sandbox. Pass `--manifest-dir` with the path that build uses, or use the unsandboxed package.

## Permanent install and sharing

Sign the extension through addons.mozilla.org and install the signed XPI. An unlisted add-on can be shared directly without appearing in the public store.

Build the package:

```bash
mkdir -p dist
npx web-ext build --source-dir plugins/firefox --artifacts-dir dist --overwrite-dest
```

Sign it with an API key and secret from the AMO Developer Hub:

```bash
export WEB_EXT_API_KEY=...
export WEB_EXT_API_SECRET=...
npx web-ext sign --source-dir plugins/firefox --artifacts-dir dist --channel=unlisted
```

Keep the signed file as `dist/docent-<version>.xpi`. Renaming the file does not affect the signature. Do not remove `META-INF/`; Firefox rejects the add-on without it.

Install it on another machine with `about:addons` → gear icon → **Install Add-on From File...**, then install the native host manifest and the `docent` and `docent-native-host` binaries on that machine as well. The extension ID stays `{ae094259-bf49-44a7-a0f0-186c52340b02}`, so keep `allowed_extensions` in the host manifest unchanged.

The XPI contains no author name or email. The manifest has no `author` field, and the signature certificate names Mozilla's signing service and the add-on ID. An author name shown on the AMO page is listing metadata on AMO, not part of the file.

Bump `"version"` in `plugins/firefox/manifest.json` for every new build and sign it again. Personal installs update by installing the new signed XPI. Add `browser_specific_settings.gecko.update_url` and host an `updates.json` only if you want Firefox to update automatically.

## Usage

Select text on a page, then click the Docent toolbar button or press `Alt+Shift+D`. The popup lists the profiles returned by `docent profiles --json`. Choose a profile and press **Run**. A profile that declares `requires = ["question"]` shows a question field before it runs.

The same profiles appear under **Docent** in the selection context menu. A profile that needs a question asks for it in the answer tab before starting the request.

The answer opens in a new foreground tab and streams as the provider returns it. When the stream finishes, the CLI renders the Markdown and the tab shows the same HTML as the qutebrowser and zathura answer pages, including MathML for TeX math. Press `d` to close the tab. Closing it, or navigating it away, cancels the request. Links in the answer open in a new tab.

The popup's **Refresh** button reloads the profile list; **Options** opens the settings page.

## CLI viewer

`docent explain notes.md --lines 12:20 --viewer firefox` opens a standalone local answer page in Firefox without the extension. It uses the same stylesheet and Markdown renderer as the answer tab, but no selection capture, native host, or extension keybindings. The Firefox extension remains the way to run a profile against a page selection. See [the browser viewer guide](viewer.md).

## Options

| Option | Default | Meaning |
|---|---|---|
| Command | `docent` | Executable name on Firefox's `PATH`, or an absolute path. |
| Config directory | platform default | Passed to the CLI as `--config-dir`. |
| Default profile | `explain` | Preselected in the popup. |
| Context mode | config default | `auto`, `full`, or `none`, passed as `--context-mode`. |
| Target language | config default | Passed as `--target-language` when set. |
| Open answers in a background tab | off | Keep focus on the page. |

The settings live in `browser.storage.local` for the extension, not in `config.toml`. Provider settings, profiles, and history still come from the CLI's own configuration.

If Firefox is launched from a desktop entry, it may not inherit your shell `PATH` or the API key environment variable. Set an absolute command in the options and export the key in the environment that starts Firefox.

## What gets captured

The extension runs one snippet in the active tab and returns:

- `selection` from `window.getSelection().toString()`.
- `document` from `document.body.innerText`, the rendered page text.
- `file` built from the page title and URL. Credentials, query parameters, and fragments are removed before the URL reaches the model.

Offsets are omitted. DOM ranges and `innerText` use different coordinate spaces, so the CLI falls back to the first exact occurrence of the selection inside the document. This is exact for short pages and can pick the wrong occurrence on very long pages.

`activeTab` grants access only after an explicit user action, so the extension does not request host permissions for every site. Reader view, view-source, the PDF viewer, and browser UI pages cannot be scripted and report an error instead.

## Answer style

The answer tab uses the same stylesheet as the qutebrowser adapter. If `<config dir>/answer.css` exists, the host reads it and the answer page applies it after the bundled styles. `docent init` writes an example file. Edit it in place to change fonts, colors, or spacing.

## Privacy

The extension declares the `websiteContent` and `browsingActivity` data collection permissions because the selected text, the rendered page text, the page title, and the cleaned page URL are sent to the provider configured in the CLI when you run a profile. Nothing is captured in the background, and no data goes anywhere until you start a request. The extension itself stores only its settings in `browser.storage.local`. Run history is written locally by the CLI.

## Troubleshooting

- `No such native application org.docent.host`: run `scripts/firefox-host-install.sh` on Linux or macOS, or check the Windows registry key, the manifest path, and the path inside the manifest. Snap and Flatpak builds look in a different directory.
- `could not start docent`: the CLI is not on Firefox's `PATH`. Set an absolute command in the options.
- `missing API key`: neither `api_key` nor the environment variable named by `api_key_env` is set in the process that started Firefox.
- `could not read this page`: the page is restricted, such as reader view, the PDF viewer, `about:` pages, or the add-ons site.
- `unexpected argument '--html'`: the installed `docent` predates the shared renderer. Reinstall or update the CLI.
- `select some text first`: select text with the mouse before running a profile that requires a selection.
- `no profiles found`: run `docent init`, then press **Refresh** in the popup.
- `This extension does not have permission to use native application org.docent.host`: the add-on ID in the extension manifest and `allowed_extensions` in the host manifest do not match.
- No answer appears: check that the request works in a terminal with `docent request --profile explain --print-prompt`, then check the provider settings in `config.toml`.
- Changes to the adapter do not appear: reload the temporary add-on from `about:debugging`.

## Limitations

- Firefox extension shortcuts are modifier combinations, not key chains. `Alt+Shift+D` replaces qutebrowser's `,de` style bindings. Users can change the shortcut at `about:addons`.
- `window.getSelection()` sees the top frame only. Iframes, shadow DOM, and PDF viewers are not supported.
- Pages with JavaScript disabled cannot be captured.
- The whole rendered page text is sent even when the CLI will trim it, so very large pages cost encoding time and memory.
- Native messaging caps messages from the host to the extension at 1 MB. Docent deltas are small, so this does not affect normal answers.
- The extension uses Manifest V2 with a persistent background page. Manifest V3 support is deferred.
- Firefox for Android does not support the native messaging host.
- The answer is a tab rather than a floating overlay.
