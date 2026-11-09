#!/usr/bin/env bash
# Manual end-to-end harness for the Firefox adapter.
#
# This starts Firefox with a temporary home, a fake docent executable, and a
# host manifest that points at the freshly built docent-native-host. It is not
# part of scripts/check.sh because it needs web-ext, a display, and manual
# interaction.
#
# Usage: tests/firefox/e2e.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if ! command -v web-ext >/dev/null 2>&1; then
    echo "web-ext is not installed; skipping"
    exit 0
fi
if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
    echo "no display available; skipping"
    exit 0
fi

if [[ ! -x "$root/target/debug/docent-native-host" ]]; then
    cargo build --manifest-path "$root/Cargo.toml" --bin docent-native-host
fi

mkdir -p "$tmp/bin" "$tmp/home/.mozilla/native-messaging-hosts" "$tmp/config"

cat > "$tmp/bin/docent" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
    profiles)
        printf '%s\n' '{"profiles":[
          {"name":"explain","description":"Explain a passage","requires":["selection"]},
          {"name":"ask","description":"Ask a question","requires":["selection","question"]}
        ]}'
        ;;
    config-path)
        printf '%s\n' "${E2E_CONFIG_DIR}"
        ;;
    request)
        cat >/dev/null
        printf '%s\n' '{"type":"delta","text":"Fake answer from the Firefox adapter."}'
        printf '%s\n' '{"type":"delta","text":"\n\n## RenderedHeading"}'
        printf '%s\n' '{"type":"done","model":"fake-model"}'
        ;;
    *)
        echo "unexpected argument: ${1:-}" >&2
        exit 2
        ;;
esac
EOF
chmod +x "$tmp/bin/docent"

cat > "$tmp/config/answer.css" <<'EOF'
:root {
  --docent-bg: #101820;
}
EOF

cat > "$tmp/home/.mozilla/native-messaging-hosts/org.docent.host.json" <<EOF
{
  "name": "org.docent.host",
  "description": "Docent test host",
  "path": "$root/target/debug/docent-native-host",
  "type": "stdio",
  "allowed_extensions": ["docent@reading-assistant"]
}
EOF

cat > "$tmp/page.html" <<'HTML'
<!doctype html>
<html><head><meta charset="utf-8"><title>Test page</title></head>
<body>
  <h1>Heading</h1>
  <p>A transformer maps a sequence to a sequence using attention.</p>
  <p>More text here.</p>
</body></html>
HTML

cat <<'EOF'
Firefox is starting with a temporary profile.

1. Select some text on the test page.
2. Click the Docent toolbar button, or press Alt+Shift+D.
3. Choose explain and press Run.
4. Check that a new tab streams the fake answer and renders RenderedHeading.
5. Press d to close the answer tab.

Close Firefox to finish.
EOF

HOME="$tmp/home" \
PATH="$tmp/bin:$PATH" \
E2E_CONFIG_DIR="$tmp/config" \
web-ext run \
    --source-dir "$root/plugins/firefox" \
    --start-url "file://$tmp/page.html"
