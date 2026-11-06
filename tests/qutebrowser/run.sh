#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."

python3 -m pytest -q \
  --cov=docent.request \
  --cov=docent.stream \
  --cov=docent.profiles \
  --cov-report=term-missing \
  --cov-fail-under=80 \
  tests/qutebrowser

ruff check qutebrowser tests/qutebrowser
ruff format --check qutebrowser tests/qutebrowser
ty check qutebrowser/docent/request.py qutebrowser/docent/stream.py qutebrowser/docent/profiles.py
