FROM debian:trixie-slim

ENV DEBIAN_FRONTEND=noninteractive \
    RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/opt/cargo/bin:${PATH}

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        appstream \
        build-essential \
        ca-certificates \
        curl \
        dbus-daemon \
        dbus-x11 \
        debhelper \
        desktop-file-utils \
        dpkg-dev \
        flatpak \
        flatpak-builder \
        gettext \
        git \
        gstreamer1.0-libav \
        gstreamer1.0-plugins-bad \
        gstreamer1.0-plugins-base \
        gstreamer1.0-plugins-good \
        libadwaita-1-dev \
        libgettextpo-dev \
        libglib2.0-bin \
        libgstreamer-plugins-base1.0-dev \
        libgstreamer1.0-dev \
        libgtk-4-dev \
        libpipewire-0.3-dev \
        libsqlite3-dev \
        libxcb-composite0-dev \
        libxcb-randr0-dev \
        libxcb-xfixes0-dev \
        libxcb1-dev \
        lintian \
        pkg-config \
        ripgrep \
        xvfb \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustup component add clippy rustfmt \
    && cargo install cargo-deny --locked \
    && rm -rf /opt/cargo/registry /opt/cargo/git

WORKDIR /workspace
