# Contributing

Thanks for helping! A few ground rules:

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
scripts/validate-xsd.sh   # if you touched scorelift-model or scorelift-musicxml
```

CI runs all of these on Linux (x64, arm64), macOS and Windows.

## Licensing

- Contributions are dual-licensed MIT OR Apache-2.0, like the project.
  Unless you say otherwise, anything you submit is licensed that way.
- **Do not copy code or model weights from AGPL/GPL projects** (for example Audiveris).
  Reading them for ideas is fine; copying is not.
- Training data must be public domain, CC0 or CC BY. No non-commercial (NC) datasets.

## Test scores

- **Never commit copyrighted sheet music.** All `*.pdf` files are git-ignored.
  Keep private test PDFs in `fixtures-private/` (also ignored).
- Public fixtures must be original, public domain, or under a free license.
  Write their source and license next to them.

## Code layout

- `scorelift-model` has no heavy dependencies. Keep it that way: every crate depends on it.
- Recognition stages talk to each other only through types in `scorelift-model`.
