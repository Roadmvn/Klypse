#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo fmt --all -- --check
bash scripts/check-translations.sh
desktop-file-validate \
  crates/klypse-app/resources/io.github.roadmvn.Klypse.desktop
appstreamcli validate --no-net \
  crates/klypse-app/resources/io.github.roadmvn.Klypse.metainfo.xml
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
xvfb-run -a cargo test --workspace --locked
cargo build --workspace --release --locked
cargo deny --all-features check

if [[ $# -gt 0 ]]; then
  bash scripts/test-debian-package.sh "$1"
fi

if [[ "${KLYPSE_VERIFY_FLATPAK:-0}" == "1" ]]; then
  bash scripts/test-flatpak.sh
fi
