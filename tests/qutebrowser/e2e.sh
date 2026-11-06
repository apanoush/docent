#!/usr/bin/env bash
# End-to-end check for the qutebrowser adapter.
#
# This starts a temporary qutebrowser instance with a fake docent executable,
# selects text with jseval, runs :docent, and reads the answer tab back. It is
# not part of the default check script because it needs a display and
# QtWebEngine. Run it manually after changes to the capture or answer-tab path.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if ! command -v qutebrowser >/dev/null 2>&1; then
    echo "qutebrowser is not installed; skipping"
    exit 0
fi
if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
    echo "no display available; skipping"
    exit 0
fi

cat > "$tmp/docent" <<'PY'
#!/usr/bin/env python3
import json
import sys
import time

if "profiles" in sys.argv:
    print(json.dumps({"profiles": [
        {"name": "explain", "description": "Explain a passage", "requires": ["selection"]},
        {"name": "ask", "description": "Ask a question", "requires": ["selection", "question"]},
    ]}))
    raise SystemExit(0)

request = json.loads(sys.stdin.read() or "{}")
question = request.get("question")
text = "Fake answer for: " + request.get("selection", "")
if question:
    text += " / Q: " + question
print(json.dumps({"type": "delta", "text": text}), flush=True)
time.sleep(0.1)
print(json.dumps({"type": "delta", "text": "\n\n## RenderedHeading"}), flush=True)
print(json.dumps({"type": "done", "model": "fake-model"}), flush=True)
PY
chmod +x "$tmp/docent"

cat > "$tmp/page.html" <<'HTML'
<!doctype html>
<html><head><meta charset="utf-8"><title>Test page</title></head>
<body><h1>Heading</h1><p id="target">A transformer maps a sequence to a sequence using attention.</p><p>More text here.</p></body></html>
HTML

cat > "$tmp/config.py" <<PY
import sys
sys.path.insert(0, "${root}/plugins/qutebrowser")
import docent
docent.setup(config, cmd=["${tmp}/docent"])
PY

log="$tmp/run.log"
timeout -s TERM 20s qutebrowser \
    -T --nowindow --no-err-windows -l debug --logfilter commands,js,misc,procs \
    -C "$tmp/config.py" "file://$tmp/page.html" \
    ':cmd-later 1500 jseval (function(){const el=document.getElementById("target");const r=document.createRange();r.selectNodeContents(el);const s=window.getSelection();s.removeAllRanges();s.addRange(r);return s.toString();})()' \
    ':cmd-later 2500 docent explain' \
    ':cmd-later 4500 jseval document.getElementById("answer") ? document.getElementById("answer").textContent : "NO ANSWER"' \
    > "$log" 2>&1 || true

if ! grep -q "Fake answer for: A transformer maps a sequence to a sequence using attention." "$log"; then
    echo "qutebrowser e2e failed; log follows" >&2
    cat "$log" >&2
    exit 1
fi
if ! grep -q "^RenderedHeading$" "$log"; then
    echo "qutebrowser e2e did not render the answer as Markdown; log follows" >&2
    cat "$log" >&2
    exit 1
fi

echo "qutebrowser e2e passed"
