# SPYDER Bone

A light desktop app (Windows and macOS) that scores ASD LabSpec 4 near-infrared scans of bone, one scan at a time,
to help decide whether destructive sampling (radiocarbon, ZooMS, isotopes) is worth it. It brings the published
Ryder et al. 2026 collagen model and related models, with clear spectral visuals, to anyone with a LabSpec 4,
standard- or high-resolution.

**Download:** https://github.com/makapania/NIR-SPYDER-Bone/releases/latest (Windows installer `.exe`; macOS `.dmg` for
Apple Silicon and Intel, macOS 13 or newer).

The macOS installer is signed and notarized by Apple. Open the disk image, drag SPYDER Bone to Applications, and open
the app. macOS may ask you to confirm the first launch of an app downloaded from the internet.

The Windows installer is still unsigned while Microsoft verifies the developer identity. If Windows shows "Windows
protected your PC", choose **More info → Run anyway** after downloading it from this repository's release page.

How to use it: [USER_GUIDE.md](USER_GUIDE.md) (also inside the app: **?** or F1).

## Layout

- `crates/spyder-core`: all the maths (ASD reader, preprocessing operators, models, checks, verdicts). No UI.
- `crates/spyder-cli`: the `spyder` command-line tool.
- `app/`: the desktop app (Tauri 2 shell + Svelte 5 UI).
- `plugins/`: bundled model, transfer and parameter files (CC-BY-4.0).
- `reference/`: the frozen Python reference implementation used for parity tests.
- `testdata/scans/`: 40 paired real scans of archaeological bone (the same bone on a standard and a high-res LabSpec 4),
  for testing and for checking the high-res transfer.

## Building

Rust (see `rust-toolchain.toml`) and Node 24. `npm ci` in `app/ui`, then from `app/src-tauri`:
`../ui/node_modules/.bin/tauri build`. Tests: `cargo test --workspace`; in `app/ui`, `npm run check` and `npm test`.
Pushing a `v*` tag builds both installers on GitHub into a draft release (`.github/workflows/release.yml`).

## Licence

Code: MIT (`LICENSE`). Model and data files in `plugins/` and the test scans in `testdata/scans/`: CC-BY-4.0
(`plugins/LICENSE.md`).
