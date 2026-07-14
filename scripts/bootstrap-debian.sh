#!/usr/bin/env bash
set -euo pipefail

packages=(
  appstream
  build-essential
  cargo
  curl
  dbus-x11
  debhelper
  desktop-file-utils
  dpkg-dev
  flatpak
  flatpak-builder
  gettext
  gstreamer1.0-libav
  gstreamer1.0-plugins-bad
  gstreamer1.0-plugins-base
  gstreamer1.0-plugins-good
  gstreamer1.0-tools
  libadwaita-1-dev
  libglib2.0-bin
  libgstreamer-plugins-base1.0-dev
  libgstreamer1.0-dev
  libgtk-4-dev
  libpipewire-0.3-dev
  libsqlite3-dev
  libxcb-composite0-dev
  libxcb-randr0-dev
  libxcb-xfixes0-dev
  libxcb1-dev
  lintian
  pkg-config
  ripgrep
  rustc
  xauth
  xvfb
)

if ! sudo -n true 2>/dev/null; then
  if docker info >/dev/null 2>&1; then
    "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/dev-container.sh" true
    exit 0
  fi

  echo "Klypse requires passwordless sudo or an accessible Docker daemon" >&2
  exit 1
fi

sudo apt-get update
sudo apt-get install -y "${packages[@]}"

if ! command -v rustup >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
    sh -s -- -y --profile minimal --default-toolchain stable
fi

source "$HOME/.cargo/env"
rustup component add clippy rustfmt

if ! command -v cargo-deny >/dev/null 2>&1; then
  cargo install cargo-deny --locked
fi
