#!/usr/bin/env bash
# Regenerates the vector test PDFs: writes the built-in fixtures as MusicXML
# and engraves them with MuseScore 4. The results are committed, so CI does
# not need MuseScore. All fixture music is original and CC0.
#
# Usage: scripts/make-vector-fixtures.sh [path/to/mscore]
set -euo pipefail

cd "$(dirname "$0")/.."
OUT=crates/scorelift-semantic/tests/fixtures/musescore
MSCORE=${1:-"/Applications/MuseScore 4.app/Contents/MacOS/mscore"}
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

mkdir -p "$OUT"
cargo run --quiet -p scorelift-musicxml --example write_fixtures -- "$TMP" >/dev/null
for f in "$TMP"/*.musicxml; do
  name=$(basename "$f" .musicxml)
  # MuseScore 4 sometimes aborts on exit after writing the file; check the file instead.
  "$MSCORE" -o "$OUT/$name.pdf" "$f" >/dev/null 2>&1 || true
  if [[ -s "$OUT/$name.pdf" ]]; then
    echo "wrote $OUT/$name.pdf"
  else
    echo "FAILED $name" >&2
    exit 1
  fi
done
"$MSCORE" --version 2>/dev/null | tail -1 > "$OUT/GENERATOR.txt" || true
