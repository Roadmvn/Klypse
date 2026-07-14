#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image="klypse-dev:trixie"
cache_root="${XDG_CACHE_HOME:-$HOME/.cache}/klypse-dev"

mkdir -p "$cache_root/cargo" "$cache_root/home"

if ! docker image inspect "$image" >/dev/null 2>&1; then
  docker build \
    --file "$root/packaging/dev/Dockerfile" \
    --tag "$image" \
    "$root/packaging/dev"
fi

docker run --rm --init \
  --user "$(id -u):$(id -g)" \
  --env CARGO_HOME=/cargo \
  --env HOME=/home/klypse \
  --env RUSTUP_HOME=/opt/rustup \
  --env DISPLAY="${DISPLAY:-}" \
  --volume "$root:/workspace" \
  --volume "$cache_root/cargo:/cargo" \
  --volume "$cache_root/home:/home/klypse" \
  --volume /tmp/.X11-unix:/tmp/.X11-unix:ro \
  --workdir /workspace \
  "$image" \
  "$@"
