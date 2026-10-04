"""Write crates/spyder-core/tests/goldens/fuzz_predictions_v1.json: the frozen Python reference's predictions on
200 seeded perturbations of the PUBLIC golden spectra (plugins/golden_spectra_public_v2.json), for every active
model and both instrument classes (high-res: the transfer chosen by the Step 4 rule). The Rust test
tests/fuzz_parity.rs regenerates the same spectra bit for bit (checked by content hash) and requires
|rust - python| <= 1e-9 + 1e-9 |python| for every output.

Public data only. Re-run whenever a plug-in file changes (the fixture records every file's SHA-256 and the Rust
test refuses a stale fixture):
    python crates/spyder-core/tests/gen/gen_fuzz_predictions.py
"""
import json
import sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
from parity_common import REPO, SR, STD, HIRES, Engine, perturb  # noqa: E402

N_CASES = 200
PLUGINS = REPO / "plugins"
OUT = REPO / "crates" / "spyder-core" / "tests" / "goldens" / "fuzz_predictions_v1.json"


def main():
    eng = Engine(PLUGINS)
    gs = SR.load_golden_spectra(PLUGINS / "golden_spectra_public_v2.json")
    bases = sorted(gs.values(), key=lambda t: t[2]["name"])
    cases = []
    for k in range(N_CASES):
        wl, x, meta = bases[k % len(bases)]
        xp, joins = perturb(wl, x, k)
        cases.append({
            "k": k,
            "base": meta["name"],
            "base_sha256": SR.spectrum_sha256(wl, x),
            "sha256": SR.spectrum_sha256(wl, xp),
            "joins": joins,
            "classes": {cls: eng.predict_all(wl, xp, joins, cls) for cls in (STD, HIRES)},
        })
    doc = {
        "format": "spyder-bone/fuzz-predictions",
        "version": 1,
        "generated_by": "crates/spyder-core/tests/gen/gen_fuzz_predictions.py (reference/spyder_ref)",
        "spyder_ref_version": SR.__version__,
        "inputs": "seeded perturbations (parity_common.perturb) of the public golden spectra; no private data",
        "golden_spectra_file": "golden_spectra_public_v2.json",
        "plugin_files": eng.files,
        "n_cases": N_CASES,
        "cases": cases,
    }
    OUT.write_text(json.dumps(doc, indent=None, separators=(",", ":"), allow_nan=False) + "\n", encoding="utf-8")
    n = sum(len(v) for c in cases for cl in c["classes"].values() for v in cl.values())
    print(f"wrote {OUT.relative_to(REPO)}: {N_CASES} spectra x 2 classes x {len(eng.models)} models, {n} values")


if __name__ == "__main__":
    main()
