#!/usr/bin/env bash
set -euo pipefail

app_id="io.github.roadmvn.Klypse"
info="$(mktemp)"
help="$(mktemp)"
trap 'rm -f "$info" "$help"' EXIT

flatpak info --user "$app_id" >"$info"
grep -Fq "ID: $app_id" "$info"

flatpak run --user --command=klypse "$app_id" --help >"$help" 2>&1
flatpak run --user --command=klypse "$app_id" capture --help >>"$help" 2>&1
flatpak run --user --command=klypse "$app_id" record --help >>"$help" 2>&1
grep -Fq 'capture <TARGET>' "$help"
grep -Fq 'area, screen, window, active-window' "$help"
grep -Fq 'video, gif' "$help"
