#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
app_id="io.github.roadmvn.Klypse"
manifest="$root/packaging/flatpak/$app_id.yml"
build_dir="$root/build/flatpak"
repository="$root/build/flatpak-repo"

if ! flatpak remote-list --user --columns=name | grep -Fxq flathub; then
  flatpak remote-add --user --if-not-exists flathub \
    https://dl.flathub.org/repo/flathub.flatpakrepo
fi

mkdir -p "$root/build"
flatpak-builder \
  --user \
  --install-deps-from=flathub \
  --disable-rofiles-fuse \
  --force-clean \
  --repo="$repository" \
  "$build_dir" \
  "$manifest"

flatpak install --user --assumeyes --reinstall "$repository" "$app_id"
