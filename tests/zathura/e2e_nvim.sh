#!/usr/bin/env bash
# End-to-end check for the zathura Neovim viewer.
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if ! command -v nvim >/dev/null 2>&1; then
    echo "nvim is not installed; skipping"
    exit 0
fi

cat > "$tmp/docent" <<'PY'
#!/usr/bin/env python3
import json
import sys

if "profiles" in sys.argv:
    print(json.dumps({"profiles": [
        {"name": "explain", "description": "Explain", "requires": ["selection"]},
    ]}))
    raise SystemExit(0)

request = json.loads(sys.stdin.read())
source = "unknown"
if "--source" in sys.argv:
    source = sys.argv[sys.argv.index("--source") + 1]
print(json.dumps({
    "type": "delta",
    "text": "Viewer answer: " + request.get("selection", "") + " (source=" + source + ")",
}), flush=True)
print(json.dumps({"type": "done", "model": "fake"}), flush=True)
PY
chmod +x "$tmp/docent"

request_dir="$tmp/docent-zathura-test"
mkdir -p "$request_dir"
python3 - "$tmp/docent" "$request_dir/request.json" <<'PY'
import json
import sys

payload = {
    "profile": "explain",
    "request": {
        "selection": "alpha",
        "document": "alpha beta",
        "file": "paper.pdf (page 1)",
    },
    "docent": sys.argv[1],
}
with open(sys.argv[2], "w", encoding="utf-8") as handle:
    json.dump(payload, handle)
PY

cat > "$tmp/test.lua" <<'LUA'
dofile(vim.env.DOCENT_ZATHURA_VIEWER)

local docent = require("docent")
local ok = vim.wait(5000, function()
  return docent._last_ui ~= nil and docent._last_ui:is_done()
end)
if not ok then
  print("TIMEOUT")
  vim.cmd("cq")
end

local lines = vim.api.nvim_buf_get_lines(docent._last_ui:buffer(), 0, -1, false)
print(table.concat(lines, "\n"))
vim.cmd("qa!")
LUA

DOCENT_ZATHURA_REQUEST="$request_dir/request.json" \
DOCENT_ZATHURA_VIEWER="$root/plugins/zathura/docent_zathura/viewer.lua" \
DOCENT_ZATHURA_NVIM="$root/plugins/nvim" \
nvim --headless --clean -u NONE \
    --cmd "lua vim.opt.runtimepath:append(vim.env.DOCENT_ZATHURA_NVIM)" \
    -c "lua dofile('$tmp/test.lua')" > "$tmp/output.txt" 2>&1 || true

if ! grep -q "Viewer answer: alpha (source=zathura)" "$tmp/output.txt"; then
    echo "zathura nvim viewer e2e failed; output follows" >&2
    cat "$tmp/output.txt" >&2
    exit 1
fi

if [ -e "$request_dir" ]; then
    echo "zathura nvim viewer did not remove the request file" >&2
    exit 1
fi

echo "zathura nvim viewer e2e passed"
