# spyder_ref: the SPYDER Bone reference engine

`spyder_ref` is the executable specification of the SPYDER Bone plug-in formats. The desktop app (Rust core) must
reproduce it to 1e-9 on every golden case. It needs **numpy only** at run time; scipy, jsonschema and pytest are used
only by the developer tools and tests (`pip install -e .[test]`).

## What is here

| Path | What |
|---|---|
| `spyder_ref/core.py` | Operators (absorbance, Savitzky–Golay, crop, segmented Gaussian blur, per-segment absorbance affine, block SNV; reserved research ops), model / consensus / transfer files, transfer selection, the hardened golden runner. Engine (op set) 1.1. |
| `spyder_ref/n2.py` | The canonical noise measure N2 (the only implementation). |
| `spyder_ref/tables.py` | Shared tables (format 2): `bands.json` (band readings, readability SD), `noise_gains.json` (B6 implied SD), the instrument registry; their golden runners. |
| `spyder_ref/gains.py` | The noise and band-readability gains, computed for any linear correction (blur and absorbance gain per detector segment) and the scan's own detector segments; "not assessed" with the reason otherwise. |
| `spyder_ref/validate_plugins.py` | Validates a whole plug-in folder as the app does on load: schemas, goldens, consensus component hashes, catalog hashes. `python -m spyder_ref.validate_plugins ../plugins` |
| `spyder_ref/golden_gen.py` | Golden generator: expected values from research paths independent of the engine (scipy Savitzky–Golay, explicit transfer loop, N2 loop, explicit blur matrices and noise covariance for the gains). |
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
- **Computed gains** (format 2 of `bands.json` and `noise_gains.json`, DECISIONS 137): the noise and readability gains
  are computed from the output's linear kernel, the correction actually applied (its blur and absorbance gain per
  detector segment, linearised) and the scan's own joins, under the tables' noise model (a correction file may carry its
  instrument's own `provenance.instrument_noise`). Nothing is keyed by a transfer file. An output without a gain is "not
  assessed" with the reason (a non-linear correction; "not available on this instrument" when a kernel or noise window
  is not inside one detector segment), never a guess; a band whose noise is unknown counts as too noisy (readable only
  when strong).
- **Noise policies for any correction** (DECISIONS 137): the per-sign B6b gates, the reduced fallbacks, the long-wave
  and soft-tier eligibility read the noise of each check's linear statistic after the correction applied
  (`gains.stat_term_gain`), against cuts fixed once in SD units in the check files; a stream read as measured keeps the
  standard-resolution gate table. A near-uncorrected correction (`checks.standard_like`: every statistic the noise
  policies read, the gate and fallback statistics, the long-wave tail reading and the soft-tier statistic, keeps its
  noise within 15% of the uncorrected noise, e.g. a standard-resolution unit's own file) is read exactly like the native
  standard-resolution scan by every noise policy: the gate table, long-wave not assessed, the soft tier always eligible,
  no S1_R2 and no second opinion. Profiles key models and the second opinion on whether the stream is the native
  standard-resolution scan, not on class names.
- **Instruments not built in** (no correction for the scan's class, the `instrument_uncalibrated` note): the band calls
  are off (`checks.band_evidence_off`; `verdict.collagen_verdict(..., uncalibrated=True)`): evidence and the ZooMS band
  pattern are "not assessed: band evidence needs this instrument built in" (the band readings stay), the model verdict
  stands, no band ZooMS line, positive-signs line or bands-too-noisy note, the ZooMS profile says Can't tell, flags carry
  `provisional` (same sort penalty), and the second-opinion note is not added (S1_R2 still shown).
- **Transfer selection** (`pick_transfer`): one version per id (the newest active); between ids, the caller's order
  (bundled before user, then by id); a file for this unit (`source_serial`, compared as text) first, even for the
  model's own class; then same class: none; then the first class-wide file. Versions are never compared across ids.
- **Correction file extras**: `provenance.visible_levels` (`fitted` | `not_fitted`; absent = fitted; heat reads the
  scan as measured when not fitted), `provenance.second_opinion_gap` (`points`, `n_scans` >= 20) and `reader_offsets`
  ({model id: wt%}, |offset| <= 5; a consensus takes its components' offsets before the median).
- **Provisional transfers**: `"provisional": true` means every result made with the transfer is labelled provisional.

## Run the tests

```
cd reference
pip install -e .[test]          # or: uv run --with numpy --with scipy --with jsonschema --with pytest python -m pytest
python -m pytest
python -m spyder_ref.validate_plugins ../plugins
```

Licence: MIT (code). The plug-in files in `../plugins` are CC-BY-4.0.
