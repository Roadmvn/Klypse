#!/usr/bin/env bash
set -euo pipefail

deb="${1:?Debian package path required}"
test -f "$deb"

temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT

dpkg-deb --contents "$deb" >"$temporary/contents"
for installed in \
  ./usr/bin/klypse \
  ./usr/share/applications/io.github.roadmvn.Klypse.desktop \
  ./usr/share/metainfo/io.github.roadmvn.Klypse.metainfo.xml \
  ./usr/share/icons/hicolor/scalable/apps/io.github.roadmvn.Klypse.svg \
  ./usr/share/glib-2.0/schemas/io.github.roadmvn.Klypse.gschema.xml \
  ./usr/share/locale/fr/LC_MESSAGES/klypse.mo
do
  grep -Fq "$installed" "$temporary/contents"
done

dpkg-deb --extract "$deb" "$temporary/root"
test -x "$temporary/root/usr/bin/klypse"
"$temporary/root/usr/bin/klypse" --help >"$temporary/help" 2>&1
"$temporary/root/usr/bin/klypse" --version >"$temporary/version" 2>&1
grep -Fq 'capture' "$temporary/help"
grep -Fq 'klypse 0.1.0' "$temporary/version"
desktop-file-validate \
  "$temporary/root/usr/share/applications/io.github.roadmvn.Klypse.desktop"
appstreamcli validate --no-net \
  "$temporary/root/usr/share/metainfo/io.github.roadmvn.Klypse.metainfo.xml"
glib-compile-schemas --strict \
  "$temporary/root/usr/share/glib-2.0/schemas"
msgunfmt \
  "$temporary/root/usr/share/locale/fr/LC_MESSAGES/klypse.mo" \
  >/dev/null
