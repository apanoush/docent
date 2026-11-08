#!/usr/bin/env bash
# Remove the Docent native messaging host manifest for Firefox.
#
# Usage: uninstall.sh [--manifest-dir DIR]
set -euo pipefail

manifest_dir="${DOCENT_FIREFOX_MANIFEST_DIR:-$HOME/.mozilla/native-messaging-hosts}"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest-dir) manifest_dir="${2:-}"; shift 2 ;;
        -h|--help)
            echo "usage: $(basename "$0") [--manifest-dir DIR]"
            exit 0
            ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

manifest="$manifest_dir/org.docent.host.json"
if [[ ! -e "$manifest" ]]; then
    echo "nothing to remove: $manifest"
    exit 0
fi

rm -f "$manifest"
echo "removed $manifest"
