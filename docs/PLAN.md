# PDF → MusicXML converter — Implementation Plan

Working name: **scorelift** (rename freely)
Language: Rust · License: MIT OR Apache-2.0 · Targets: macOS (arm64/x86_64), Windows (x64), Linux (x64/arm64)

---

## 1. What the sample PDFs tell us

| File | Pages | Embedded images | Content |
|---|---|---|---|
| Perfect Strangers Vocal.pdf | 3 | 20 | Single treble staff, melody + lyrics, tuplets, ties, multi-measure rests, section labels |
| Perfect Strangers Organ.pdf | 4 | 33 | Single treble staff, 2–3 note chords, ties, grace notes, dynamics, "let ring" |
| Wish You Were Here (UG).pdf | 2 | 8 | Single treble staff, key sig (G), dense 4–6 note chords, 16ths, dotted rhythms, "let ring" lines |

What I found:

- **All three are browser-printed Ultimate Guitar pages** (Creator: Mozilla/Skia and Edge/Quartz). The notation is **not vector**. Each staff system is a separate **lossless (Flate) raster image, 1980 px wide** (~260 dpi at print size). Only the title, artist and "Tuning:" line exist as real PDF text.
- So the main engine has to be **Optical Music Recognition (OMR)** on clean, computer-rendered images. That's the easier kind of OMR: no paper noise and no skew, and the engraving style is the same throughout.
- **One image per system** gives us layout segmentation for free. Extract the embedded images at native resolution rather than re-rendering the page.
- The hard cases are dense chord stacks (Wish You Were Here), beams, ties versus slurs, tuplets and lyrics.
- ⚠️ The samples are copyrighted. Use them only as **local, private test fixtures**. Never commit them to the public repo. The public test suite uses synthetic or public-domain scores.

## 1b. Lessons from Audiveris

[Audiveris](https://github.com/Audiveris/audiveris) is the leading open-source OMR program. It is written in Java and licensed **AGPL-3.0**, so we borrow **ideas only, never code or model weights**. Otherwise our MIT/Apache license breaks.

| Audiveris idea | What we adopt |
|---|---|
| **20 named pipeline steps** (LOAD, BINARY, SCALE, GRID, HEADERS, STEM_SEEDS, BEAMS, LEDGERS, HEADS, STEMS, REDUCTION, CUE_BEAMS, TEXTS, MEASURES, CHORDS, CURVES, SYMBOLS, LINKS, RHYTHMS, PAGE). The user can run up to any step. | Same model: an explicit `Step` enum, `run_until(step)`, and per-step debug output. **Improvement:** we snapshot the state after each step, so re-running from step N doesn't restart from LOAD. Audiveris re-runs everything from the start when you go back. |
| **SCALE step** measures line thickness, interline, *small* interline (cue/grace notes), beam thickness and stem thickness from run-length histograms. Beam thickness is a known weak spot, so it can be overridden per book. | One `Scale` struct holding all of these. Every later step thresholds in interline units, not pixels. Beam thickness can be overridden in the CLI and GUI. |
| **SIG (Symbol Interpretation Graph)**, one per system. Nodes are candidate interpretations ("inters") with a *grade*. Edges are relations: **support** (e.g. head–stem raises both grades), **exclusion** (two readings of the same ink compete), and neutral ones (chord containment). REDUCTION and LINKS resolve conflicts globally. | This is the biggest change to the plan. Stage 5 keeps **several hypotheses** in a graph (`petgraph`) instead of committing greedily, then reduces the graph. Detector confidences become the starting grades. The rhythm check in stage 6 feeds back into the graph as more support or exclusion. |
| **Mixed recognisers**: special-purpose code for lines, morphological closing for beams, **template matching for noteheads**, a small neural net for fixed-size symbols, Tesseract for text | Our samples are clean computer renders, so **template matching for heads plus morphology for beams and stems can carry the first version without deep learning**. Neural nets come in for robustness. This takes the training pipeline off the critical path. |
| **Book / Sheet / Page / Score** model: sheets are processed independently, then gathered cheaply at book level. An indented system or a page with no music starts a new movement. | Same hierarchy in `scorelift-model`. Sheets run in parallel (`rayon`). Movements are detected the same way. Export one `.mxl` per movement, or one combined file. |
| **`.omr` project file** that stores all inters, so manual corrections survive. The editor works on inters and relations. | A `.scorelift` project file (zip of serde JSON + images). The GUI edits the SIG/IR, not the exported XML, so you can re-export at any time. |
| **Trainer that learns from samples**: user-validated glyphs become training data | Optional, opt-in export of GUI corrections as labelled samples into `training/`. Over time this tunes the models to real-world input. |
| **Known Audiveris limits**: no handwriting; only triplet and 6-tuplet tuplets; opposed stems sometimes merge into one; Tesseract misreads chord-symbol accidentals; Java runtime required | Places to do better: general n-tuplets in RHYTHMS; a dedicated stem-split check; a pure-Rust OCR (`ocrs`) with a music-text post-processor (♯/♭, chord names); a single native binary. |

**Use Audiveris as a benchmark.** Run its CLI (`-batch -export`) on the same eval set and track our accuracy against it in `eval/`.

## 2. Product scope

**v1.0**
- Input: PDF (vector or raster), plus PNG/JPG/TIFF.
- Output: `.musicxml` (uncompressed) and `.mxl` (compressed), MusicXML 4.0.
- Notation covered: single staff and grand staff (piano); treble, bass and alto clefs; key and time signatures; notes, rests, chords, accidentals, dots, beams, flags, ties, slurs, tuplets, grace notes, multi-measure rests, repeats and barlines, dynamics, tempo, rehearsal and section text, lyrics, title, composer.
- Two front ends that share one core:
  - **CLI** (`scorelift convert in.pdf -o out.mxl`) for batch work.
  - **Desktop GUI** to review, correct and export. OMR is never 100% accurate, so a quick correction UI is what makes the app usable.
- Fully **offline**: models ship inside the app, with no cloud calls.

**Out of scope for v1** (planned for later): guitar TAB staves, handwritten scores, orchestral scores with many staves, chord-symbol and fretboard diagrams, and percussion notation.

## 3. Architecture

```
            ┌────────────────────────────────────────────────────────────┐
 PDF/img ──►│ 1 Ingest ─► 2 Preprocess ─► 3 Layout/Staves ─► 4 Symbols  │
            │                                                     │      │
            │      7 Export ◄── 6 Validate/Repair ◄── 5 Semantics ◄┘      │
            └────────────────────────────────────────────────────────────┘
                 ▲ library crate `scorelift-core` (no UI deps)
        ┌────────┴────────┐
   scorelift-cli     scorelift-gui (review/correct)
```

### Cargo workspace

```
crates/
  scorelift-core/      # public API: convert(path, opts) -> Score; pipeline orchestration
  scorelift-pdf/       # PDF parsing, image extraction, page rendering, text layer
  scorelift-vision/    # preprocessing, staff detection/removal, connected components
  scorelift-detect/    # NN inference (symbol detection + segmentation), model loading
  scorelift-semantic/  # symbols → musical model (pitch, rhythm, voices, measures)
  scorelift-model/     # Score IR: Part/Measure/Voice/Note/... (serde-serializable)
  scorelift-musicxml/  # IR → MusicXML 4.0 / .mxl writer (+ reader for tests)
  scorelift-cli/
  scorelift-gui/
training/              # Python (PyTorch) — dataset generation, training, ONNX export
eval/                  # ground truth + metrics harness
```

### Pipeline stages

**1. Ingest** (`scorelift-pdf`)
- **Done in M1, pure Rust:** `lopdf` reads the PDF; we walk each page's content stream (tracking the transform matrix through `q`/`Q`/`cm` and nested forms), decode every image XObject (Flate/LZW/DCT; Gray/RGB/CMYK/ICC/Indexed/Separation; 1–16 bit; SMask) to grayscale and record where it is drawn. No native library to ship.
- Rendering pages to pixels (needed only for vector pages without the music-font fast path, and for exotic image codecs such as JBIG2/CCITT/JPX in scans) is deferred. If needed, add `pdfium-render` behind a feature flag.
- For each page, classify how the music is stored:
  - **Raster-embedded** (like the samples): extract the image XObjects at native resolution, keep their placement on the page, and treat each one as a system candidate.
  - **Vector with music font** (MuseScore, Sibelius, Finale, Dorico, LilyPond exports): music glyphs are text runs in SMuFL fonts (Bravura, Leland, Emmentaler…), and staff lines are paths. → **fast path** (step 4b) with near-perfect accuracy.
  - **Scan**: render at 300–400 dpi.
- Read the PDF text layer for title, composer and tuning, so those don't need OCR.

**2. Preprocess** (`scorelift-vision`, crates `image` and `imageproc`)
- Convert to grayscale, then apply adaptive binarization (Sauvola). Clean renders can use Otsu.
- For scans only: deskew (Hough on long horizontal runs) and denoise.
- Normalize scale to a fixed **staff-space ≈ 20 px**. This makes the models independent of input DPI.

**3. Layout and staff detection**
- Estimate staff-line thickness and spacing from vertical run-length histograms (the classic Fujinaga method).
- Detect staff lines from horizontal projections and line tracking, then group them into 5-line staves and staves into systems.
- Detect barlines (vertical runs that span the staff) to split each system into measures.
- Keep a copy of the image with staff lines removed for symbol detection, and keep the original for the GUI.

**4a. Symbol recognition (raster)** (`scorelift-detect`)
- Hybrid model, trained in Python and exported to **ONNX**:
  1. **Semantic segmentation** (small U-Net) to separate staff lines, noteheads, stems, beams, and other symbols. Thin structures like stems and beams segment better than they detect as boxes.
  2. **Object detector** (YOLO-style, anchor-free) for discrete glyphs: clefs, accidentals, rests, flags, dots, time-signature digits, dynamics, fermatas, tuplet numbers, and so on.
  3. **Text**: section labels, tempo text, lyrics and "let ring" go through the `ocrs` crate (pure-Rust OCR, runs on ONNX via `rten`).
- Inference runtime: **`ort` (ONNX Runtime)**, statically linked so it runs fast on CPU everywhere. Keep **`tract`** (pure Rust) as a fallback feature flag so a build can avoid native dependencies entirely.
- Rule-based geometry fixes up the network output: noteheads snap to staff positions; stems attach to noteheads; beams count to give durations.

**4b. Symbol recognition (vector fast path)**
- Map SMuFL code points to symbols (for example U+E0A4 = black notehead) and take their positions from the PDF glyph coordinates. Staff lines come from path objects.
- No machine learning is needed. The output is the same `Symbol` list, so stages 5–7 are shared with the raster path.

**5. Semantic reconstruction** (`scorelift-semantic`). This is the core music logic.
- Every candidate becomes an **inter** in a per-system interpretation graph, using Audiveris's SIG idea (§1b). Support and exclusion relations are added by the rules below. A **reduction** pass then keeps the most consistent set of inters, rather than trusting each detection on its own.
- **Pitch** = staff position + active clef + key signature + accidentals in the measure (accidentals carry through the measure and across ties).
- **Duration** = notehead type (filled, hollow, whole) + stem + flag count or beam count + dots + tuplet brackets.
- **Chords** are noteheads that share a stem. Seconds sit on alternating sides of the stem.
- **Voices** follow stem direction and overlapping onsets. v1 supports up to 2 voices per staff.
- **Attach** ties and slurs to notes (by curvature and endpoints), dynamics and articulations to the nearest note, and lyrics to notes by x-alignment and syllable hyphens.
- Read multi-measure rests (the thick bar with a number) and expand them.

**6. Validate and repair**
- Each measure's durations must add up to the time signature. If they don't, try the most likely fixes in order: a missed dot, a misread flag or beam, an implied tuplet, a pickup measure. Each candidate is scored by detector confidence.
- Check that all staves in a system have the same number of measures.
- Flag anything left unresolved as **low confidence**. The GUI highlights those measures.

**7. Export** (`scorelift-musicxml`)
- A hand-written serializer using `quick-xml`; `.mxl` is the same XML packed with the `zip` crate and a `META-INF/container.xml`.
- Write `<divisions>` as the LCM of the durations used, and include `<print>` and system breaks so the original layout is kept.
- In CI, validate the output against the MusicXML 4.0 XSD and import it into MuseScore 4 headless as a smoke test.

### GUI

- **Recommendation: Tauri 2.** The Rust backend calls `scorelift-core` directly. The frontend is HTML/TS using **Verovio (WASM, LGPL)** or **OpenSheetMusicDisplay (BSD)** to show the recognized score next to the original page.
- Why not pure-Rust egui? Rendering engraved music notation is a large project in itself. Verovio and OSMD already do it, and side-by-side checking is the main thing the GUI is for.
- Features:
  - Open a PDF and watch progress.
  - Split view: original image with detected symbols drawn over it, next to the rendered result.
  - Click a symbol to fix it (pitch up or down, duration, delete, add).
  - Low-confidence measures are highlighted.
  - Export `.musicxml`, `.mxl` or MIDI.
- The CLI stays free of the webview, so headless Linux servers need nothing extra.

## 4. Training the models (`training/`, Python. Ships only the `.onnx` files.)

- **Synthetic data does most of the work:** take public MusicXML corpora (PDMX, OpenScore Lieder, the IMSLP/Mutopia public-domain subset), render them with **Verovio** and **MuseScore CLI** in several fonts (Bravura, Leland, Petaluma, Emmentaler), and use the renderer's own SVG output for pixel-perfect labels.
- **Match the target style:** add a renderer configuration that copies Ultimate Guitar's engraving, plus mild augmentations (scale, blur, JPEG artifacts, and paper texture and rotation for the scan path).
- **Public datasets:** DeepScoresV2 (detection, CC BY 4.0) for pre-training. Avoid datasets with non-commercial licenses (for example MUSCIMA++, CC BY-NC-SA) if you want the weights to stay freely licensed.
- **Prior art to study, not copy:** Audiveris (AGPL, so no code reuse under MIT; see §1b), oemer (MIT), homr, and TrOMR / Sheet Music Transformer for end-to-end staff→sequence models. An end-to-end transformer per staff is worth testing as an experiment branch after the MVP.
- Model budget: under 60 MB total, under 2 s per page on a laptop CPU.

## 5. Evaluation

- `eval/`: ground-truth MusicXML for each test score. For your three PDFs, transcribe them once in MuseScore and keep them private.
- Metrics:
  - Note-level precision and recall on (onset, pitch, duration).
  - Measure-level exact-match rate.
  - Symbol error rate via `musicdiff`-style alignment.
  - Edits needed per page (a proxy for GUI effort).
- CI runs the public eval set on every pull request and fails if accuracy drops.

## 6. Cross-platform build and distribution

- GitHub Actions matrix: `macos-14` (arm64), `macos-13` (x64), `windows-latest`, `ubuntu-22.04` (x64 + arm64).
- CLI: `cargo-dist` produces tarballs, a shell/PowerShell installer and Homebrew/Scoop manifests.
- GUI: the Tauri bundler produces a `.dmg` (signed and notarized), an `.msi`/NSIS installer, and `.AppImage` + `.deb`, plus a Flathub manifest later.
- Bundled native pieces: the ONNX Runtime (static), and PDFium only if the optional renderer feature is added. The models sit in the app resources and are found through a `models/` lookup path.
- Linux GUI depends on WebKitGTK and Windows needs WebView2. Both are standard; document them.

## 7. Milestones

| # | Milestone | Deliverable / exit criteria |
|---|---|---|
| M0 ✅ | Project setup (1 wk) | Workspace, CI matrix builds on all 3 OSes, license, CONTRIBUTING, Score IR + MusicXML writer with hand-built test scores that pass XSD validation |
| M1 ✅ | Ingest + vision (2 wk) | `scorelift debug in.pdf` extracts the system images from all 3 samples and writes overlay PNGs with staves and barlines detected (100% of staves/measures found on the samples). **Result:** 61/61 staves, 274 measures; spot-checked systems match the printed measure numbers |
| M2 ✅ | Vector fast path (2 wk) | Vector PDFs exported from MuseScore convert end to end at ≥98% note accuracy. Proves stages 5–7 without ML. **Result:** 100% of notes and rests on all three fixtures engraved by MuseScore 4 (pitch, duration, onset), plus clefs, keys, times, ties, slurs, tuplets, grace notes, dots, beams, lyrics, dynamics, hairpins, tempo, rehearsal marks, repeats, endings, multi-measure rests, title/composer/part names |
| M3 | Classical recognisers + SIG (2 wk) | Template matching for noteheads, clefs and accidentals; morphology for stems and beams; interpretation graph + reduction. No ML yet (the Audiveris approach) |
| M4 | Monophonic raster MVP (2 wk) | **Perfect Strangers Vocal** → MusicXML with ≥90% note accuracy (lyrics optional); compared against Audiveris on the same file |
| M4b | Training pipeline v1 (3 wk, runs alongside M4–M6) | Synthetic data generator + segmentation and detector models exported to ONNX. Their outputs become extra inters with grades, so the robustness gains don't require rewriting the pipeline |
| M5 | Chords and polyphony (3 wk) | **Organ** and **Wish You Were Here** ≥85% note accuracy; ties, tuplets, grace notes, dynamics; validate/repair stage in place |
| M6 | GUI (3 wk) | Tauri app: open, review, correct, export; low-confidence highlighting |
| M7 | Breadth (3 wk) | Grand staff, bass clef, lyrics OCR, repeats, scanned PDFs |
| M8 | Release 1.0 (1 wk) | Signed installers for all 3 OSes, docs, model card, public eval report |

Total: about 20 weeks for one developer. M2 and M3 can run in parallel.

## 8. Key risks and mitigations

| Risk | Mitigation |
|---|---|
| Dense chord stacks (Wish You Were Here) are misread | Segmentation-based notehead counting + geometric stem grouping; dedicated synthetic chord-heavy training set |
| Rhythm errors break measure totals | The validate/repair stage, plus a GUI that points straight to bad measures |
| Native deps (PDFium, ONNX Runtime) are hard to package | Prebuilt binaries + a `tract` pure-Rust fallback feature; CI builds on every OS from day one |
| Model and dataset licensing | Use only CC BY / public-domain data; publish a model card that lists the data sources |
| Copyrighted sample scores | Keep them in a git-ignored `fixtures-private/`; public tests use public-domain scores |

## 9. Immediate next steps

1. Install the Rust toolchain (`rustup`), plus Python 3.11 for `training/`.
2. Run `git init` and create the workspace skeleton (M0).
3. Write the Score IR and MusicXML writer first. Every later stage builds on them.
4. ~~Build the M1 debug overlay tool against the three sample PDFs.~~ Done.

### M1 lessons
- **Find staff lines on the gray image, not the binary one.** A one-pixel anti-aliased line that straddles two rows is half-dark in each and disappears after thresholding.
- **Measure each row's darkness against the lightest row nearby**, and grow each line from its darkest row. A fixed cutoff merges lines with nearby dense chords.
- **Allow one missing interior line**, and keep only the darkest row of a band that is too thick. Multi-measure rest bars and beams lying on a line otherwise hide it.
- **A barline stops exactly at the outer staff lines and has no notehead beside its ends.** Stems that span the whole staff fail one of these tests.

### M2 lessons
- **Music fonts map glyphs to SMuFL code points through ToUnicode.** MuseScore (Leland) does; tempo notes use the newer metronome range U+ECA0–ECB7 in the text font, not U+E1D0.
- **Programs split lines into pieces.** Staff lines are drawn per measure group, and system barlines per gap between staves; join touching collinear segments before analysing.
- **Lyric hyphens can be drawn lines, not "-" text**, and directions like "let ring" share the lyric font: tell them apart by phrase shape (a direction contains spaces) and alignment under noteheads.
- **Dots are found to the right of a chord up to the next event further right**, not the next event in x order, which may belong to another voice at the same position.
- **Dorico 6 (Bravura) works after one addition:** it draws noteheads with Bravura's *optional* glyphs (U+F4BE "noteheadBlackOversized" etc.), which are font-specific. A table generated from Bravura's SMuFL metadata (`glyphsWithAlternates`) maps 111 alternates to their standard symbols. Dorico also types lyric hyphens as "-" text, draws hairpins as one filled outline, offsets ending hooks by half a line width, and places dynamics below lyrics, close to the next system. All three fixtures: 100% of notes and rests.
- **Untested so far:** Sibelius and Finale exports; other fonts' optional glyphs (the installed SMuFL metadata in `/Library/Application Support/SMuFL/Fonts/<font>/` could be read at run time); cross-staff beams; more than two voices per staff; let-ring ties.

