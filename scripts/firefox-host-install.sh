#!/usr/bin/env bash
# Install the Docent native messaging host manifest for Firefox.
#
# Usage: install.sh [--force] [--host PATH] [--manifest-dir DIR]
set -euo pipefail

host=""
manifest_dir="${DOCENT_FIREFOX_MANIFEST_DIR:-}"
force=""
root="$(cd "$(dirname "$0")/.." && pwd)"

usage() {
    cat <<EOF
usage: $(basename "$0") [--force] [--host PATH] [--manifest-dir DIR]

  --host PATH          docent-native-host executable (default: PATH lookup,
                       then ~/.cargo/bin/docent-native-host)
  --manifest-dir DIR   where to write org.docent.host.json
                       (default on Linux: ~/.mozilla/native-messaging-hosts,
                       on macOS: ~/Library/Application Support/Mozilla/NativeMessagingHosts)
  --force              overwrite an existing manifest

This script installs the Linux and macOS manifest. Windows needs a registry key;
follow docs/firefox.md.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --force) force="1"; shift ;;
        --host) host="${2:-}"; shift 2 ;;
        --manifest-dir) manifest_dir="${2:-}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

platform="$(uname -s)"
case "$platform" in
    MINGW*|MSYS*|CYGWIN*)
        echo "this script cannot register the manifest on Windows; follow docs/firefox.md" >&2
        exit 1
        ;;
esac

if [[ -z "$manifest_dir" ]]; then
    case "$platform" in
        Darwin) manifest_dir="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts" ;;
        *) manifest_dir="$HOME/.mozilla/native-messaging-hosts" ;;
    esac
fi

if [[ -z "$host" ]]; then
    if command -v docent-native-host >/dev/null 2>&1; then
        host="$(command -v docent-native-host)"
    elif [[ -x "$HOME/.cargo/bin/docent-native-host" ]]; then
        host="$HOME/.cargo/bin/docent-native-host"
    else
        echo "could not find docent-native-host; install it with 'cargo install --path .' or pass --host" >&2
        exit 1
    fi
fi

if [[ ! -x "$host" ]]; then
    echo "not an executable file: $host" >&2
    exit 1
fi

host="$(cd "$(dirname "$host")" && pwd)/$(basename "$host")"
manifest="$manifest_dir/org.docent.host.json"

if [[ -e "$manifest" && -z "$force" ]]; then
    echo "$manifest already exists; pass --force to replace it" >&2
    exit 1
fi

mkdir -p "$manifest_dir"
cat > "$manifest" <<EOF
{
  "name": "org.docent.host",
  "description": "Native messaging host for the Docent Firefox adapter",
  "path": "$host",
  "type": "stdio",
  "allowed_extensions": ["docent@reading-assistant"]
}
EOF

echo "wrote $manifest"
echo
echo "Next steps:"
echo "  1. Open about:debugging in Firefox, choose 'This Firefox', then 'Load Temporary Add-on'"
echo "     and select $root/plugins/firefox/manifest.json."
echo "  2. Select text on a page and use the Docent toolbar button or Alt+Shift+D."
echo
echo "A temporary add-on disappears when Firefox restarts. For a permanent install,"
echo "sign the extension through addons.mozilla.org."
