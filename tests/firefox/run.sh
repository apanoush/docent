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

# The host scripts must use the manifest directory Firefox reads on each platform.
script_test="$(mktemp -d)"
trap 'rm -rf "$script_test"' EXIT

mkdir -p "$script_test/bin" "$script_test/home" "$script_test/host"
cat > "$script_test/bin/uname" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "${FAKE_UNAME:-Linux}"
EOF
cat > "$script_test/host/docent-native-host" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$script_test/bin/uname" "$script_test/host/docent-native-host"

linux_manifest="$script_test/home/.mozilla/native-messaging-hosts/org.docent.host.json"
macos_manifest="$script_test/home/Library/Application Support/Mozilla/NativeMessagingHosts/org.docent.host.json"
custom_manifest="$script_test/custom/org.docent.host.json"

host_install() {
    local platform="$1"
    shift
    HOME="$script_test/home" PATH="$script_test/bin:$PATH" FAKE_UNAME="$platform" \
        scripts/firefox-host-install.sh --host "$script_test/host/docent-native-host" "$@" >/dev/null
}

host_uninstall() {
    local platform="$1"
    shift
    HOME="$script_test/home" PATH="$script_test/bin:$PATH" FAKE_UNAME="$platform" \
        scripts/firefox-host-uninstall.sh "$@" >/dev/null
}

host_install Linux
[[ -f "$linux_manifest" ]] || {
    echo "install script did not write the Linux manifest" >&2
    exit 1
}

host_install Darwin
[[ -f "$macos_manifest" ]] || {
    echo "install script did not write the macOS manifest" >&2
    exit 1
}

host_install Linux --manifest-dir "$script_test/custom"
[[ -f "$custom_manifest" ]] || {
    echo "install script ignored --manifest-dir" >&2
    exit 1
}
host_uninstall Linux --manifest-dir "$script_test/custom"
[[ ! -e "$custom_manifest" ]] || {
    echo "uninstall script ignored --manifest-dir" >&2
    exit 1
}

if host_install MINGW64_NT-10.0 2>/dev/null; then
    echo "install script must refuse to run on Windows" >&2
    exit 1
fi

host_uninstall Linux
[[ ! -e "$linux_manifest" ]] || {
    echo "uninstall script did not remove the Linux manifest" >&2
    exit 1
}

host_uninstall Darwin
[[ ! -e "$macos_manifest" ]] || {
    echo "uninstall script did not remove the macOS manifest" >&2
    exit 1
}
