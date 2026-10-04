# SPYDER Bone

A light desktop app (Windows and macOS) that scores ASD LabSpec 4 near-infrared scans of bone, one scan at a time,
to help decide whether destructive sampling (radiocarbon, ZooMS, isotopes) is worth it. It brings the published
Ryder et al. 2026 collagen model and related models, with clear spectral visuals, to anyone with a LabSpec 4,
standard- or high-resolution.

Status: early development. No release yet.

## Layout

- `crates/spyder-core`: all the maths (ASD reader, preprocessing operators, models, checks, verdicts). No UI.
- `crates/spyder-cli`: the `spyder` command-line tool.
- `app/`: the desktop app (Tauri 2 shell + Svelte 5 UI).
- `plugins/`: bundled model, transfer and parameter files (CC-BY-4.0).
- `reference/`: the frozen Python reference implementation used for parity tests.

## Licence

Code: MIT (`LICENSE`). Model and data files in `plugins/`: CC-BY-4.0 (`plugins/LICENSE.md`).
