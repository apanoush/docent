#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

python3 -m pytest -q \
  --cov=docent.request \
  --cov=docent.stream \
  --cov=docent.profiles \
  --cov=docent.markdown \
  --cov-report=term-missing \
  --cov-fail-under=80 \
  tests/qutebrowser

ruff check plugins/qutebrowser tests/qutebrowser
ruff format --check plugins/qutebrowser tests/qutebrowser
ty check plugins/qutebrowser/docent/request.py plugins/qutebrowser/docent/stream.py plugins/qutebrowser/docent/profiles.py plugins/qutebrowser/docent/markdown.py
