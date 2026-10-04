# spyder_ref: the SPYDER Bone reference engine

`spyder_ref` is the executable specification of the SPYDER Bone plug-in formats. The desktop app (Rust core) must
reproduce it to 1e-9 on every golden case. It needs **numpy only** at run time; scipy, jsonschema and pytest are used
only by the developer tools and tests (`pip install -e .[test]`).

## What is here

| Path | What |
|---|---|
| `spyder_ref/core.py` | Operators (absorbance, Savitzky–Golay, crop, segmented Gaussian blur, per-segment absorbance affine, block SNV; reserved research ops), model / consensus / transfer files, transfer selection, the hardened golden runner. Engine (op set) 1.1. |
| `spyder_ref/n2.py` | The canonical noise measure N2 (the only implementation). |
| `spyder_ref/tables.py` | Shared tables: `bands.json` (band readings, readability gains), `noise_gains.json` (B6 implied SD), the instrument registry; their golden runners. |
| `spyder_ref/validate_plugins.py` | Validates a whole plug-in folder as the app does on load: schemas, goldens, consensus component hashes, catalog hashes. `python -m spyder_ref.validate_plugins ../plugins` |
| `spyder_ref/golden_gen.py` | Golden generator: expected values from research paths independent of the engine (scipy Savitzky–Golay, explicit transfer loop, N2 loop). |
| `spyder_ref/export.py` | Exporter for new linear collagen models (`export_regression`), with goldens and validation. |
| `spyder_ref/schemas/` | JSON Schemas (draft 2020-12) for model (incl. `consensus`), transfer, bands, noise gains, instrument registry, reference set, catalog and golden-spectra files. |
| `oracle/` | The end-to-end Python oracle (Steps 1–10 of the plan). |
| `tests/` | Tests on public data only. |

## Conventions

- Spectra are float64 on a 1 nm grid 350–2500 nm (2151 channels); reflectance = sample DN / white-reference DN.
- Every number in a plug-in file is finite JSON (`allow_nan=False`); the loader rejects NaN/Infinity and 1e999.
- A plug-in file is identified by `id@version` and the SHA-256 of its bytes. MAJOR changes predictions, MINOR adds
  goldens or statistics, PATCH is text only.
- Goldens: at least 3 cases, at least one with the intermediate feature vector (regression), engine tolerance 1e-9
  absolute + 1e-9 relative (a file may tighten, never loosen). Golden spectra must sit in the plug-in's own folder.
- **Consensus kind** (engine 1.1): `value` = median of an odd number of components; each component is a shipped model
  folded exactly into one linear functional of the processed spectrum and identified by id, version and file SHA-256.
  Outputs `value` and `component:<name>`.
- **Transfer keys**: noise gains and readability gains are keyed by `"none"` or the SHA-256 of the transfer file
  actually applied. A key with no entry means "not assessed", never a guess.
- **Provisional transfers**: `"provisional": true` means every result made with the transfer is labelled provisional.

## Run the tests

```
cd reference
pip install -e .[test]          # or: uv run --with numpy --with scipy --with jsonschema --with pytest python -m pytest
python -m pytest
python -m spyder_ref.validate_plugins ../plugins
```

Licence: MIT (code). The plug-in files in `../plugins` are CC-BY-4.0.
