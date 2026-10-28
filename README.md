# scorelift

Open-source, offline converter from PDF sheet music to MusicXML, written in Rust.
Runs on macOS, Windows and Linux.

> **Status: milestone M0.** The score model and the MusicXML writer work.
> PDF recognition is not implemented yet. See [docs/PLAN.md](docs/PLAN.md).

## Workspace

| Crate | Purpose | Status |
|---|---|---|
| `scorelift-model` | Score IR (parts, measures, voices, events), validation, fixtures | ✅ M0 |
| `scorelift-musicxml` | IR → MusicXML 4.0 (`.musicxml`) and compressed `.mxl` | ✅ M0 |
| `scorelift-cli` | `scorelift` command-line tool | ✅ skeleton |
| `scorelift-core` | Pipeline orchestration (`convert`) | stub |
| `scorelift-pdf` | PDF ingest: image extraction, rendering, text layer | stub (M1) |
| `scorelift-vision` | Binarization, scale, staff/barline detection | stub (M1) |
| `scorelift-detect` | Symbol recognisers (templates, later ONNX models) | stub (M3) |
| `scorelift-semantic` | Interpretation graph, pitch/rhythm/voices | stub (M3) |

## Build

```bash
cargo build --release
cargo test --workspace
```

Try the writer with a built-in demo score, then open the file in MuseScore:

```bash
cargo run -p scorelift-cli -- demo feature-tour -o out/feature-tour.mxl
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
