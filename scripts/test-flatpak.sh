#!/usr/bin/env bash
set -euo pipefail
export LC_ALL=C

app_id="io.github.roadmvn.Klypse"
info="$(mktemp)"
help="$(mktemp)"
permissions="$(mktemp)"
trap 'rm -f "$info" "$help" "$permissions"' EXIT

flatpak info --user "$app_id" >"$info"
grep -Fq "ID: $app_id" "$info"

flatpak info --user --show-permissions "$app_id" >"$permissions"
grep -Eq '^shared=.*ipc' "$permissions"
grep -Eq '^sockets=.*(fallback-x11|x11)' "$permissions"
grep -Eq '^sockets=.*wayland' "$permissions"
grep -Eq '^devices=.*dri' "$permissions"
grep -Eq '^filesystems=.*xdg-pictures' "$permissions"
! grep -Eq '(^|[=;])network(;|$)' "$permissions"
! grep -Eq '(^|[=;])home(;|$)' "$permissions"

flatpak run --user --command=sh "$app_id" -c \
  'test -f /app/share/locale/fr/LC_MESSAGES/klypse.mo'

flatpak run --user --command=klypse "$app_id" --help >"$help" 2>&1
flatpak run --user --command=klypse "$app_id" capture --help >>"$help" 2>&1
flatpak run --user --command=klypse "$app_id" record --help >>"$help" 2>&1
grep -Fq 'capture <TARGET>' "$help"
# Assert each accepted value on its own rather than one formatted line: clap
# lays the list out differently once the values carry descriptions.
for value in area screen window active-window video gif; do
  grep -Fq -- "- ${value}:" "$help"
done
