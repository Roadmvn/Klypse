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

# GStreamer pipeline tests share process-wide plugin state. Keep the media
# crate in one bounded, serial run so independent GIF pipelines cannot race.
timeout --kill-after=30s 15m \
  cargo test --workspace --exclude klypse-media --locked
timeout --kill-after=30s 10m \
  cargo test -p klypse-media --locked -- --test-threads=1

# Re-run only tests that exercise a real X11/GTK display. The full workspace
# suite above already covers display-independent media pipelines, and running
# those a second time under Xvfb can leave GStreamer workers blocking the gate.
timeout --kill-after=30s 10m xvfb-run -a \
  cargo test --locked -p klypse-platform \
    --test hotkey_mapping \
    --test x11_capture \
    --test x11_recording_source
timeout --kill-after=30s 10m xvfb-run -a \
  cargo test --locked -p klypse-app \
    --lib \
    --test accessibility \
    --test annotated_sharing \
    --test desktop_content \
    --test editor_view \
    --test gallery_scale \
    --test recording_ui \
    --test recovery_ui \
    --test region_overlay \
    -- --test-threads=1
cargo deny --all-features check

if [[ $# -gt 0 ]]; then
  bash scripts/test-debian-package.sh "$1"
fi

if [[ "${KLYPSE_VERIFY_FLATPAK:-0}" == "1" ]]; then
  bash scripts/test-flatpak.sh
fi
