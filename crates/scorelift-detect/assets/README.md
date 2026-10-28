# Symbol templates

`templates.bin` holds bitmaps of the SMuFL glyphs listed in
`src/templates.rs`, rendered from **Bravura** (© Steinberg Media
Technologies GmbH, SIL Open Font License 1.1) at 24 px per staff space.
Regenerate with:

```bash
cargo run -p scorelift-detect --example make_templates -- /path/to/Bravura.otf
```
