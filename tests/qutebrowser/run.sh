#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

python3 -m pytest -q \
  --cov=docent_qutebrowser \
  --cov-report=term-missing \
  --cov-fail-under=80 \
  tests/qutebrowser

ruff check plugins/qutebrowser tests/qutebrowser
ruff format --check plugins/qutebrowser tests/qutebrowser
ty check plugins/qutebrowser
