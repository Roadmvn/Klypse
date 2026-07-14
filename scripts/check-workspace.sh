#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
required=(
  Cargo.toml
  rust-toolchain.toml
  crates/klypse-domain/Cargo.toml
  crates/klypse-platform/Cargo.toml
  crates/klypse-storage/Cargo.toml
  crates/klypse-media/Cargo.toml
  crates/klypse-image/Cargo.toml
  crates/klypse-app/Cargo.toml
)

for file in "${required[@]}"; do
  test -f "$root/$file" || {
    echo "missing $file" >&2
    exit 1
  }
done
