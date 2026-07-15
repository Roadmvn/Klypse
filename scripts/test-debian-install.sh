#!/usr/bin/env bash
set -euo pipefail

deb="${1:?Debian package path required}"
image="${KLYPSE_DEBIAN_TEST_IMAGE:-debian:trixie-slim}"
deb="$(realpath "$deb")"
test -f "$deb"

docker run --rm \
  --volume "$deb:/tmp/klypse.deb:ro" \
  "$image" \
  sh -euxc '
    export DEBIAN_FRONTEND=noninteractive
    install_log=/tmp/klypse-install.log
    if ! apt-get update --quiet=2 >"$install_log" 2>&1 ||
       ! apt-get install --yes --quiet=2 --no-install-recommends /tmp/klypse.deb >>"$install_log" 2>&1
    then
      cat "$install_log"
      exit 1
    fi
    command -v gdbus
    command -v gst-inspect-1.0
    gst-inspect-1.0 pipewiresrc >/dev/null
    klypse --help >/dev/null
    klypse --version
  '
