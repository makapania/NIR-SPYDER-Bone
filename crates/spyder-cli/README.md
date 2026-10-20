# spyder (command-line tool)

```
spyder read     <file.asd | folder> [--json] [--recursive]
spyder validate <plug-in file | folder> [--json] [--user] [--pin id@version]...
spyder predict  <file.asd | folder> --class std|hires|CLASS [--json] [--recursive]
                [--plugins DIR] [--user-plugins DIR] [--pin id@version]...
spyder analyse  <file.asd | folder> --class std|hires|CLASS [--profile radiocarbon|isotopes|zooms]
                [--csv out.csv] [--json] [--sort] [--recursive] [--plugins DIR] [--user-plugins DIR] [--pin ...]
```

Arguments are OS strings (paths need not be UTF-8). Recursive walks never follow symlinked or junction
directories (no cycles).

Reads ASD LabSpec 4 `.asd` files and reports, per scan: whether it is accepted (the supported-input
matrix of PLAN section 3 Step 1), its header fields, N2 noise per window (on the as-measured scan) and the
acquisition checks B1-B8. A folder is read non-recursively unless `--recursive` is given; files are sorted
by path; hidden files and macOS `._*` companions are skipped; the extension match is case-insensitive.

Exit codes: `0` success (also when some files are rejected: that is a result, not an error),
`1` the path does not exist or cannot be listed, `2` usage error.

## Accepted input (v1)

`as8`, data type RAW (sample DN with a stored white reference, reference flag set), float64 data,
dark-correction flag set, 2151 channels from 350 nm in 1 nm steps, header joins exactly 1000 and 1800 nm,
white reference finite and > 0 in 1000-2450 nm, and the whole file present (header, both blocks and at least
the 212-byte v8 trailer). Anything else is rejected with a typed reason; nothing is guessed.
Reflectance R = sample DN / white-reference DN.

## JSON output (`--json`): schema `spyder-bone/read`, version 2

The document is stable: fields are only ever added (with a `schema_version` bump if any meaning changes). Version 2
(other ASD instruments): `format`, `joins_source` and the joins used in `splices_nm`; the B7 value `max_r_1001_1800`
became `max_r_swir1` (with `swir1_nm`).
Field order below is the order printed. Numbers that are undefined (NaN/inf) are `null`.

```
{
  "schema": "spyder-bone/read",
  "schema_version": 2,
  "spyder_core_version": "0.1.0",
  "engine": "1.1",                       // preprocessing operator-set version of spyder-core
  "files": [ FileReport, ... ]
}
```

### FileReport

| field | type | meaning |
|---|---|---|
| `path` | string | path as given/listed |
| `file_name` | string | file name only |
| `acceptance.status` | `"accepted"` \| `"not_scored"` \| `"rejected"` | `not_scored` = a white-reference or dark save |
| `acceptance.label` | string | plain words: `accepted`, `reference scan (not scored)`, `unsupported`, `truncated`, `invalid`, `not asd`, `io` |
| `acceptance.error_kind` | null \| `"truncated"` \| `"not_asd"` \| `"unsupported"` \| `"invalid"` \| `"io"` | rejected files only |
| `acceptance.reason` | null \| string | the reader's message |
| `kind` | null \| `"sample"` \| `"white_reference_save"` \| `"dark_save"` | null when rejected |
| `header` | null \| object | see below |
| `timestamps` | null \| object | see below |
| `splices_nm` | null \| [number, number] | per-file detector joins |
| `segments` | null \| [{`name`, `start`, `end`, `first_nm`, `last_nm`}] | VNIR / SWIR1 / SWIR2; `[start, end)` channel indices; a join belongs to the lower segment |
| `reflectance` | null \| {`min_r_1000_2450`, `max_r_1000_2450`, `mean_r_1000_2450`} | summary of R |
| `n2` | null \| {`2000_2100`, `1500_1600`, `1500_1550`, `2300_2400`} | canonical N2 (units 1e-5 absorbance; window `[lo, hi)` of second-difference centres) |
| `checks` | [CheckResult] | always B1, B2, B3, B4, B5, B6, B6b, B7, B8 in that order |
| `worst_outcome` | `"pass"` \| `"note"` \| `"check"` \| `"unusable"` | highest assessed outcome |
| `warnings` | [string] | reader facts for the log (e.g. header join not at the white-reference DN jump; trailer holds data) |

### header

`version` ("as8"), `program_version`, `file_version`, `dark_corrected`, `data_type`, `data_format`,
`first_wavelength_nm`, `wavelength_step_nm`, `channels`, `integration_time_ms`, `fore_optic`,
`calibration_series`, `serial` (offset 400), `ad_bits`, `flags` (bytes 421-424), `dark_averages`,
`reference_averages`, `sample_averages`, `instrument_type`, `swir1_gain`, `swir2_gain`, `swir1_offset`,
`swir2_offset`, `splice1_nm` (offset 444), `splice2_nm` (offset 448).

### timestamps

| field | meaning |
|---|---|
| `spectrum_ole`, `reference_ole` | raw OLE automation dates from the reference header (days since 1899-12-30, local clock) |
| `spectrum_local`, `reference_local` | the same as `YYYY-MM-DDTHH:MM:SS` (nearest second), null if implausible |
| `acquired_tm_local` | the header `struct tm` (offset 160), local |
| `tm_isdst` | its daylight-saving flag |
| `reference_time_t`, `dark_time_t` | header time_t values (offsets 187, 182), UTC seconds |
| `utc_offset_minutes` | local minus UTC: `reference_ole` (local) minus `reference_time_t` (UTC), rounded to 15 min; null if they disagree by > 120 s from any quarter hour |
| `reference_age_s` | sample time minus white-reference time, seconds |

### CheckResult

```
{
  "id": "B5",
  "name": "very dark spot",
  "assessment": {"status": "assessed"}
              | {"status": "not_assessed", "reason": "..."}
              | {"status": "gated", "reason": "..."},
  "outcome": null | "pass" | "note" | "check" | "unusable",   // null unless assessed
  "values": { ... numbers behind the outcome ... },
  "message": null | "plain-words message"                    // only when outcome is above pass
}
```

The assessment status is always separate from the result: a check that did not run is never "pass".
B6 (noise as prediction uncertainty) is per model and transfer: `spyder read` reports it `not_assessed` and
`spyder predict` reports each reading's implied noise SD (`noise_sd_pct`); the B6 rule runs in the Phase 3 pipeline.

| id | rule (PLAN section 3 Step 2) | values |
|---|---|---|
| B1 | the reader's supported-input matrix; failure is Unusable | `error_kind`, `error` |
| B2 | raw DN >= 65,000 or >= 3 identical consecutive maxima of a detector segment; Unusable inside 1000-2450 nm, Note outside | `max_raw_dn`, `saturated_channels`, `saturated_channels_1000_2450`, `first_saturated_nm` |
| B3 | R <= 0 (or not finite) in 1000-2450 nm: Unusable | `channels_r_le_0_1000_2450`, `first_nm` |
| B4 | mean R(1000-2450) 0.95-1.05 with SD < 0.03, or mean < 0.03: Unusable; reference saves not assessed | `mean_r_1000_2450`, `sd_r_1000_2450` |
| B5 | mean R(1000-2450) < 0.08: Note | `mean_r_1000_2450` |
| B6 | a verdict cut (0.5, 3) within 2.6 x the implied SD (gain x N2) of the reading: Check, never Unusable (DECISIONS 109); pipeline only, `not_assessed` here | (pipeline record: `models.<m>.B6`) |
| B6b | N2(2000-2100) > 60: Note; contaminant signs gated | `n2_2000_2100`, `cut`, `signs_gated` |
| B7 | max R over SWIR1 (1001-1800 nm on LabSpec 4; the scan's own joins) > 1: Note only | `max_r_swir1`, `swir1_nm` |
| B8 | relative splice step at each header join > 5 %: Note; > 15 %: Check | `join1_nm`, `join2_nm`, `step1_pct`, `step2_pct` |

For a rejected file B2-B8 are `not_assessed` ("the file could not be read (B1)"); for a reference save,
B3-B8 are `not_assessed` ("reference scan (not scored)").


## `spyder validate`

Loads plug-in files exactly as the app does: strict JSON (finite numbers only), `.npy` sidecars (plain relative
path inside the plug-in folder, <= 16 MiB, SHA-256 of the file, little-endian float64, C order, finite), schema-level
validation per kind, goldens run on load (>= 3 cases, at least one with the feature vector for regressions, required
outputs per kind, no unknown outputs, tolerance <= 1e-9 abs and rel, zero-comparison runs rejected), cross-file
rules (consensus components by id, version and file SHA-256; the models the `noise_gains.json` goldens name by file
SHA-256; bands named by engine checks; models named by profiles; catalog entries), then precedence and pins. Engine-check and
profile goldens are checked for content and run by the Phase 3 engine. A folder is validated as the bundled folder
(a failing `active` file is a startup error) unless `--user` is given; a single file is validated in the context
of its own folder. Exit codes: `0` everything loads, `1` otherwise, `2` usage. `--json`: schema
`spyder-bone/validate` v1 (`files[]` with `path`, `sha256`, `format`, `id`, `version`, `status`, `state`
(`loaded` | `disabled` | `ignored`), `selected`, `error_kind`, `error`, `golden_checks`, `golden_failed`,
`golden_worst_ratio`, `golden_cases_deferred`, `notes`; `notes`, `startup_errors`, `ok`).

## `spyder predict`

Reads `.asd` files and runs every selected active model and CONS3 on each scan. `--class` is the user's
Standard / High-res switch for LabSpec 4 files (`std` / `hires`; DECISIONS 50), or any class the instrument registry
lists (`plugins/instruments.json`); a file another instrument wrote (Trek-type `as7`, whose detector joins are not
1000/1800 nm, unlike a LabSpec 4's `as7` or `as8`; FieldSpec 3 legacy reflectance) is predicted as that instrument's
class, whatever `--class` says (each file reports its `instrument_class`;
DECISIONS 137). A serial listed under the other class only adds a gentle note. For each model the correction follows
the Step 4 rule over the registry's selection (one version per id, bundled files before user files, then by id; the
first match wins, so versions are never compared across ids): a file for this unit (`source_serial`, compared as
text) wins, even for the model's own class (a lab's own standard-resolution unit file); else same class or
`also_valid_for`: none; else the first class-wide correction; none available: the model runs on the scan as measured,
with a note that it is uncalibrated. A correction's `reader_offsets` are added to the readings it makes (a consensus:
to its components before the median; reported as `reader_offset` / `component_offsets`). Each result carries the
model's `id`, `version` and file `sha256`, the correction's `id`, `version`, `sha256` and `provisional` flag, the assessment status (grid eligibility), the signed unrounded
prediction (`value`, `linear`, `T2`, `Q`, `domain_ratio`, `domain_level`, CONS3 `components`), `noise_sd_pct` (computed
for the transfer applied and the scan's own detector joins) and `noise_status` (`assessed`, or why not).
The bundled plug-in folder is `--plugins`, else `$SPYDER_PLUGINS_DIR`, else `plugins/` next to (or up to four levels
above) the executable, else the repository's `plugins/`; the user folder is `--user-plugins` or
`$SPYDER_USER_PLUGINS_DIR`. `--json`: schema `spyder-bone/predict` v1.

## `spyder analyse`

The full per-scan pipeline (PLAN Steps 1-10, Step 3 order), a port of the frozen Python oracle
(`reference/oracle`): acquisition checks B1-B8 and B6b on the as-measured scan; the instrument class from `--class`
(as for `predict`: another instrument's file is scored as its own class, `class_source` `file_format`, and with no
correction installed for it carries the note key `instrument_uncalibrated` and has the band calls off: evidence and
the ZooMS band check "not assessed: band evidence needs this instrument built in", the model verdict stands, flags carry
`provisional`, no `second_opinion_differs`; a serial listed under the other class adds the note key
`serial_class_mismatch`); the stream correction (for LabSpec 4 high-res the shipped high-res ->
standard-res file, provisional); the noise policies for the correction applied (DECISIONS 137: per-sign B6b gates, the
reduced fallbacks, long-wave and soft-tier eligibility from each statistic's noise after the correction, in SD units;
the native standard-resolution scan keeps today's table and behaviour, and so does a near-uncorrected correction,
one that keeps every noise-policy statistic's noise within 15% of uncorrected, such as a standard-resolution unit's own
file); band readability; B9 (or its truncated variant);
the profile's models with B6; organic evidence; the ZooMS band pattern; the signs (plaster, wax, ester, burnt) and
C1; then the verdict of the chosen profile (rule L2 with flat-bands-lead, +D, lift / blocked lift; ZooMS by
pattern). Flags never change a verdict. The app shows the radiocarbon / isotopes verdict for every analysis: where the
ZooMS band check calls a scan better, those profiles add a ZooMS line (note keys `zooms_better_good`,
`zooms_better_protein`, `zooms_better_1545`; DECISIONS 80 amended); the `zooms` profile stays for the CLI. Text output:
per scan the verdict and the rule step that set it, CONS3, the evidence level, the ZooMS pattern, the notes shown, the
flags. `--csv FILE`: one row per scan (UTF-8 with BOM, CRLF; every number unrounded, Python `repr`; note and flag
columns hold keys; `zooms_line` the ZooMS line's key; `zooms_band_check.*` the ZooMS band check). `--json`: schema
`spyder-bone/analyse` v1 with every scan's full `record` (the oracle's structure) and its analysis `manifest`
(input SHA-256, effective configuration, dependency hashes, result revision). `--sort`: most promising first (verdict group, then scans with a contaminant flag after the clean ones of
the group, `sort_penalty` 1; DECISIONS 99; then the profile's value).
A bundled startup error stops scoring (exit 1, reasons on stderr), as for `predict`.
