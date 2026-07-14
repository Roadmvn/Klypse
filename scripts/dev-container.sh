#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
image="klypse-dev:trixie"
cache_root="${XDG_CACHE_HOME:-$HOME/.cache}/klypse-dev"
dockerfile="$root/packaging/dev/Dockerfile"
dockerfile_hash="$(sha256sum "$dockerfile" | cut -d' ' -f1)"
image_hash="$(docker image inspect --format '{{ index .Config.Labels "io.github.roadmvn.klypse.dev-dockerfile-sha256" }}' "$image" 2>/dev/null || true)"

mkdir -p "$cache_root/cargo" "$cache_root/home"

if [[ "$image_hash" != "$dockerfile_hash" ]]; then
  docker build \
    --file "$dockerfile" \
    --label "io.github.roadmvn.klypse.dev-dockerfile-sha256=$dockerfile_hash" \
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
