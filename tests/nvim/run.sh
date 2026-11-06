#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
nvim --headless --clean -l tests/nvim/test_docent.lua
