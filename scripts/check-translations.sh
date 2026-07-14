#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT

pot="$temporary/klypse.pot"
merged="$temporary/fr.po"
untranslated="$temporary/untranslated.po"
fuzzy="$temporary/fuzzy.po"

cd "$root"
xgettext \
  --from-code=UTF-8 \
  --language=C++ \
  --keyword=gettext \
  --files-from=crates/klypse-app/resources/po/POTFILES.in \
  --output="$pot"
msgmerge \
  --quiet \
  --no-fuzzy-matching \
  --output-file="$merged" \
  crates/klypse-app/resources/po/fr.po \
  "$pot"
touch "$untranslated"
msgattrib --untranslated --no-obsolete "$merged" --output="$untranslated"
touch "$fuzzy"
msgattrib --only-fuzzy --no-obsolete "$merged" --output="$fuzzy"

untranslated_count="$(grep -c '^msgid ' "$untranslated" || true)"
fuzzy_count="$(grep -c '^msgid ' "$fuzzy" || true)"
if [[ "$untranslated_count" -ne 0 || "$fuzzy_count" -ne 0 ]]; then
  echo "French catalog is incomplete or fuzzy" >&2
  msgattrib --untranslated --no-obsolete "$merged" >&2 || true
  msgattrib --only-fuzzy --no-obsolete "$merged" >&2 || true
  exit 1
fi

msgfmt \
  --check \
  --check-format \
  --output-file="$temporary/klypse.mo" \
  "$merged"
