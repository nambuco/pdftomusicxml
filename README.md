# scorelift

Open-source, offline converter from PDF sheet music to MusicXML, written in Rust.
Runs on macOS, Windows and Linux.

> **Status: milestone M2.** PDFs exported from notation software convert to
> MusicXML. Tested with MuseScore 4; other programs that use a SMuFL music
> font (Dorico, recent Sibelius and Finale) should work but are untested.
> Scanned and image-only PDFs are analysed (staves, barlines, measures) but
> their notes are not recognised yet. See [docs/PLAN.md](docs/PLAN.md).

## Workspace

| Crate | Purpose | Status |
|---|---|---|
| `scorelift-model` | Score IR (parts, measures, voices, events), validation, fixtures | ✅ M0 |
| `scorelift-musicxml` | IR → MusicXML 4.0 (`.musicxml`) and compressed `.mxl` | ✅ M0 |
| `scorelift-cli` | `scorelift` command-line tool (`debug`, `demo`, `convert`) | ✅ M1 |
| `scorelift-core` | Pipeline orchestration (`analyze`, `convert`) | ✅ vector path |
| `scorelift-pdf` | PDF ingest: images, glyphs (with Unicode/SMuFL), vector paths (pure Rust) | ✅ M2 |
| `scorelift-vision` | Binarization, scale, staves, barlines, measures, overlays | ✅ M1 |
| `scorelift-detect` | Symbol recognisers (templates, later ONNX models) | stub (M3) |
| `scorelift-semantic` | Vector reader: SMuFL glyphs + lines → `Score` (pitch, rhythm, voices, ties, lyrics, …) | ✅ M2 |

## Build

```bash
cargo build --release
cargo test --workspace
```

Convert a PDF exported from notation software:

```bash
cargo run --release -p scorelift-cli -- convert score.pdf -o score.mxl
```

Try the writer with a built-in demo score, then open the file in MuseScore:

```bash
cargo run -p scorelift-cli -- demo feature-tour -o out/feature-tour.mxl
```

See what the recogniser finds in a PDF or PNG/JPEG. This writes one overlay
image per system (staff lines red, barlines blue, measures shaded) and a
`layout.json` with all measurements:

```bash
cargo run --release -p scorelift-cli -- debug score.pdf -o out/debug
```

Validate all fixtures against the official MusicXML 4.0 XSD (needs `xmllint` and `curl`):

```bash
scripts/validate-xsd.sh
```

### Synced folders

If the checkout lives in a cloud-synced folder (Synology Drive, Dropbox, iCloud…), keep
build output out of it, because `target/` grows to several GB:

```bash
export CARGO_TARGET_DIR="$HOME/.cache/scorelift-target"
```

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
