#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${1:-$root/build/debian}"
version="$(cd "$root" && dpkg-parsechangelog -SVersion)"
architecture="$(dpkg --print-architecture)"

mkdir -p "$output_dir"
stage_root="$(mktemp -d "$output_dir/stage.XXXXXX")"
source_dir="$stage_root/klypse-${version%%-*}"
trap 'rm -rf "$stage_root"' EXIT
mkdir -p "$source_dir"

tar \
  --directory "$root" \
  --exclude='./.git' \
  --exclude='./.worktrees' \
  --exclude='./build' \
  --exclude='./target' \
  --create --file - . \
  | tar --directory "$source_dir" --extract --file -

container_source="/workspace/${source_dir#"$root/"}"
"$root/scripts/dev-container.sh" \
  sh -c "cd '$container_source' && dpkg-buildpackage -us -uc -b"

package="$stage_root/klypse_${version}_${architecture}.deb"
test -f "$package"
install -m 0644 "$package" "$output_dir/$(basename "$package")"
printf '%s\n' "$output_dir/$(basename "$package")"
