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
const manifest = JSON.parse(fs.readFileSync("plugins/firefox/manifest.json", "utf8"));
if (manifest.browser_specific_settings.gecko.id !== "docent@reading-assistant") {
  throw new Error("the add-on ID must match scripts/firefox-host-install.sh");
}
if (!manifest.permissions.includes("nativeMessaging")) {
  throw new Error("nativeMessaging permission is missing");
}
'

if command -v web-ext >/dev/null 2>&1; then
    web-ext lint --source-dir plugins/firefox
else
    echo "web-ext is not installed; skipping manifest lint"
fi
