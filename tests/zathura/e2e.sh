#!/usr/bin/env bash
# End-to-end check for the zathura adapter.
#
# This creates a small PDF, puts a selection on the clipboard, runs the
# adapter with a fake terminal and a fake docent, and checks the streamed
# answer. It needs a display and a clipboard tool.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
tmp="$(mktemp -d)"

cleanup() {
    if command -v wl-copy >/dev/null 2>&1 && [ -n "${WAYLAND_DISPLAY:-}" ]; then
        wl-copy --clear --primary >/dev/null 2>&1 || true
    elif command -v xclip >/dev/null 2>&1 && [ -n "${DISPLAY:-}" ]; then
        printf '' | xclip -selection primary >/dev/null 2>&1 || true
    fi
    rm -rf "$tmp"
}
trap cleanup EXIT

for tool in ps2pdf python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "missing $tool; skipping"
        exit 0
    fi
done

clipboard_tool=""
if command -v wl-copy >/dev/null 2>&1 && [ -n "${WAYLAND_DISPLAY:-}" ]; then
    clipboard_tool="wl"
elif command -v xclip >/dev/null 2>&1 && [ -n "${DISPLAY:-}" ]; then
    clipboard_tool="xclip"
elif command -v xsel >/dev/null 2>&1 && [ -n "${DISPLAY:-}" ]; then
    clipboard_tool="xsel"
else
    echo "no clipboard tool; skipping"
    exit 0
fi

cat > "$tmp/page.ps" <<'PS'
%!PS
/Helvetica findfont 24 scalefont setfont
72 720 moveto (Page one alpha beta gamma.) show
showpage
/Helvetica findfont 24 scalefont setfont
72 720 moveto (Page two delta epsilon.) show
showpage
PS
ps2pdf "$tmp/page.ps" "$tmp/paper.pdf"

cat > "$tmp/docent" <<'PY'
#!/usr/bin/env python3
import json
import sys

if len(sys.argv) > 1 and sys.argv[1] == "extract":
    pages = sys.argv[sys.argv.index("--pages") + 1]
    if pages != "1:1":
        print(f"unexpected page range: {pages}", file=sys.stderr)
        raise SystemExit(2)
    print("Page one alpha beta gamma.")
    raise SystemExit(0)

request = json.loads(sys.stdin.read())
print(json.dumps({"type": "delta", "text": "Fake answer: " + request["selection"]}), flush=True)
print(json.dumps({"type": "done", "model": "fake"}), flush=True)
PY
chmod +x "$tmp/docent"

cat > "$tmp/fake-terminal" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [ "${1:-}" != "-e" ]; then
    echo "expected -e" >&2
    exit 2
fi
shift
"$@" > "$FAKE_TERMINAL_OUTPUT" 2>&1
SH
chmod +x "$tmp/fake-terminal"

export FAKE_TERMINAL_OUTPUT="$tmp/answer.txt"
case "$clipboard_tool" in
    wl) printf 'alpha beta' | wl-copy --primary ;;
    xclip) printf 'alpha beta' | xclip -selection primary ;;
    xsel) printf 'alpha beta' | xsel --primary --input ;;
esac

"$root/plugins/zathura/docent-zathura" explain "$tmp/paper.pdf" 1 \
    --terminal "$tmp/fake-terminal" \
    --docent "$tmp/docent" \
    --context-pages 0 </dev/null

for _ in $(seq 1 50); do
    grep -q "Fake answer: alpha beta" "$FAKE_TERMINAL_OUTPUT" 2>/dev/null && break
    sleep 0.1
done

if ! grep -q "Fake answer: alpha beta" "$FAKE_TERMINAL_OUTPUT" 2>/dev/null; then
    echo "zathura e2e failed; output follows" >&2
    cat "$FAKE_TERMINAL_OUTPUT" >&2 2>/dev/null || true
    exit 1
fi

echo "zathura e2e passed"
