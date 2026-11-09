#!/usr/bin/env bash
# Remove the Docent native messaging host manifest for Firefox.
#
# Usage: uninstall.sh [--manifest-dir DIR]
set -euo pipefail

manifest_dir="${DOCENT_FIREFOX_MANIFEST_DIR:-}"

usage() {
    cat <<EOF
usage: $(basename "$0") [--manifest-dir DIR]

  --manifest-dir DIR   directory holding org.docent.host.json
                       (default on Linux: ~/.mozilla/native-messaging-hosts,
                       on macOS: ~/Library/Application Support/Mozilla/NativeMessagingHosts)

This script removes the Linux and macOS manifest. Windows also needs the
registry key removed; follow docs/firefox.md.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest-dir) manifest_dir="${2:-}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    esac
done

platform="$(uname -s)"
case "$platform" in
    MINGW*|MSYS*|CYGWIN*)
        echo "this script cannot remove the registry key on Windows; follow docs/firefox.md" >&2
        exit 1
        ;;
esac

if [[ -z "$manifest_dir" ]]; then
    case "$platform" in
        Darwin) manifest_dir="$HOME/Library/Application Support/Mozilla/NativeMessagingHosts" ;;
        *) manifest_dir="$HOME/.mozilla/native-messaging-hosts" ;;
    esac
fi

manifest="$manifest_dir/org.docent.host.json"
if [[ ! -e "$manifest" ]]; then
    echo "nothing to remove: $manifest"
    exit 0
fi

rm -f "$manifest"
echo "removed $manifest"
