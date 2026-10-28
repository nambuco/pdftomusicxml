#!/usr/bin/env bash
# Validates every built-in fixture against the official MusicXML 4.0 XSD.
# The schema is downloaded once into .cache/ (it is not redistributed here).
# Requires: curl, xmllint (libxml2).
set -euo pipefail

cd "$(dirname "$0")/.."
SCHEMA_DIR=.cache/musicxml-4.0
BASE=https://raw.githubusercontent.com/w3c/musicxml/v4.0/schema
OUT_DIR=$(mktemp -d)
trap 'rm -rf "$OUT_DIR"' EXIT

if [[ ! -f "$SCHEMA_DIR/musicxml.xsd" ]]; then
  mkdir -p "$SCHEMA_DIR"
  for f in musicxml.xsd xml.xsd xlink.xsd; do
    curl -sSfL "$BASE/$f" -o "$SCHEMA_DIR/$f"
  done
  # Resolve the schema imports locally instead of over the network.
  sed -i.bak -E \
    -e 's#schemaLocation="[^"]*/xml\.xsd"#schemaLocation="xml.xsd"#' \
    -e 's#schemaLocation="[^"]*/xlink\.xsd"#schemaLocation="xlink.xsd"#' \
    "$SCHEMA_DIR/musicxml.xsd"
  rm -f "$SCHEMA_DIR/musicxml.xsd.bak"
fi

cargo run --quiet -p scorelift-musicxml --example write_fixtures -- "$OUT_DIR" >/dev/null
# Also check what the vector reader recovers from the engraved fixtures.
for pdf in crates/scorelift-semantic/tests/fixtures/musescore/*.pdf; do
  name=$(basename "$pdf" .pdf)
  cargo run --quiet -p scorelift-cli -- convert "$pdf" -o "$OUT_DIR/$name.from-pdf.musicxml" >/dev/null
done

status=0
for f in "$OUT_DIR"/*.musicxml; do
  if xmllint --noout --nonet --schema "$SCHEMA_DIR/musicxml.xsd" "$f" 2>"$OUT_DIR/err.txt"; then
    echo "ok    $(basename "$f")"
  else
    echo "FAIL  $(basename "$f")"
    sed 's/^/      /' "$OUT_DIR/err.txt"
    status=1
  fi
done
exit $status
