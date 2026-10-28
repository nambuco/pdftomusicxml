#!/usr/bin/env bash
# Regenerates the engraved test files: writes the built-in fixtures as MusicXML
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

# Raster versions at 280 dpi (staff space about 19 px, like typical web
# scores) for the image recogniser: in Leland (MuseScore's default) and in
# Bravura (via scripts/bravura.mss). MuseScore names pages <name>-1.png.
render_png() { # <out dir> [style args…]
  local dir=$1; shift
  mkdir -p "$dir"
  for f in "$TMP"/*.musicxml; do
    name=$(basename "$f" .musicxml)
    "$MSCORE" -r 280 "$@" -o "$dir/$name.png" "$f" >/dev/null 2>&1 || true
    if [[ -s "$dir/$name-1.png" ]]; then
      mv "$dir/$name-1.png" "$dir/$name.png"
      echo "wrote $dir/$name.png"
    else
      echo "FAILED $dir/$name.png" >&2
      exit 1
    fi
  done
}
render_png crates/scorelift-detect/tests/fixtures/musescore
render_png crates/scorelift-detect/tests/fixtures/musescore-bravura --style scripts/bravura.mss
