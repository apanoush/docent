#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

python3 -m pytest -q \
  --cov=docent_zathura \
  --cov-report=term-missing \
  --cov-fail-under=80 \
  tests/zathura

ruff check plugins/zathura tests/zathura
ruff format --check plugins/zathura tests/zathura
ty check --extra-search-path plugins/qutebrowser plugins/zathura/docent_zathura
python3 plugins/zathura/docent-zathura --help >/dev/null

if command -v nvim >/dev/null 2>&1; then
    tests/zathura/e2e_nvim.sh
fi
