#!/usr/bin/env bash
# Unit tests and static checks for the Firefox adapter.
set -euo pipefail
cd "$(dirname "$0")/../.."

if ! command -v node >/dev/null 2>&1; then
    echo "node is required for the Firefox adapter tests" >&2
    exit 1
fi

node --test tests/firefox/

# The Firefox answer page ships a copy of the qutebrowser stylesheet.
if ! cmp -s plugins/qutebrowser/docent/answer.css plugins/firefox/answer/answer.css; then
    echo "plugins/firefox/answer/answer.css differs from the qutebrowser stylesheet" >&2
    echo "copy plugins/qutebrowser/docent/answer.css over it" >&2
    exit 1
fi

node -e '
const fs = require("fs");
const path = require("path");

function jsFiles(directory) {
  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...jsFiles(full));
    } else if (entry.name.endsWith(".js")) {
      files.push(full);
    }
  }
  return files;
}

const manifest = JSON.parse(fs.readFileSync("plugins/firefox/manifest.json", "utf8"));
if (manifest.browser_specific_settings.gecko.id !== "docent@reading-assistant") {
  throw new Error("the add-on ID must match scripts/firefox-host-install.sh");
}

const code = jsFiles("plugins/firefox").map((file) => fs.readFileSync(file, "utf8")).join("\n");
const required = [
  ["browser.menus", "menus"],
  ["browser.contextMenus", "contextMenus"],
  ["browser.notifications", "notifications"],
  ["browser.storage", "storage"],
];
for (const [needle, permission] of required) {
  if (code.includes(needle) && !manifest.permissions.includes(permission)) {
    throw new Error(`${needle} is used but the "${permission}" permission is missing`);
  }
}
if (/(connectNative|sendNativeMessage)/.test(code) && !manifest.permissions.includes("nativeMessaging")) {
  throw new Error("native messaging is used but the nativeMessaging permission is missing");
}
if (code.includes("tabs.executeScript")
    && !manifest.permissions.includes("activeTab")
    && !manifest.permissions.includes("tabs")) {
  throw new Error("tabs.executeScript is used without activeTab or tabs");
}
'

if command -v web-ext >/dev/null 2>&1; then
    web-ext lint --source-dir plugins/firefox
else
    echo "web-ext is not installed; skipping manifest lint"
fi
