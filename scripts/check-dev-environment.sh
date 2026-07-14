#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runner="$root/scripts/dev-container.sh"

test -x "$runner" || {
  echo "missing executable scripts/dev-container.sh" >&2
  exit 1
}

"$runner" cargo --version | grep -q '^cargo '
