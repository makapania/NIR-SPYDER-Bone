"""Generate crates/spyder-core/tests/goldens/operators_v1.json from spyder_ref and n2.py on SYNTHETIC inputs.

Everything here is analytic or seeded-random: no measured spectrum, no private data. The Rust tests
(tests/operators.rs) check:
  * SG weights: bitwise for the table combinations (both delta 1 and 2), ~1e-16 relative for the QR fallback;
  * numpy summation helpers: bitwise against np.sum / np.mean / np.std;
  * operator and chain outputs: |got - want| <= 1e-12 + 1e-12 |want| (PLAN gate: 1e-9);
  * N2 on synthetic noisy spectra against n2.py;
  * cases that must fail (reserved operators, a block SNV spanning a join).

Run:  <dasp venv>/python.exe crates/spyder-core/tests/gen/gen_operator_goldens.py
"""
import hashlib
import json
import sys
from pathlib import Path

import numpy as np

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
from refpath import import_spyder_ref, REPO  # noqa: E402

sr, ref_dir = import_spyder_ref()
N2_DIR = REPO / "planning" / "work" / "phase0a" / "common"
sys.path.insert(0, str(N2_DIR))
from n2 import n2 as n2_ref  # noqa: E402

rng = np.random.default_rng(20261003)
OUT = REPO / "crates" / "spyder-core" / "tests" / "goldens" / "operators_v1.json"


def fl(a):
    return [float(v) for v in np.asarray(a, float).ravel()]


# ------------------------------------------------------------------ synthetic spectra
def synth_bone_like(wl, steps=(0.02, -0.015), joins=(1000.0, 1800.0), noise=2e-4, seed=1):
    """Smooth 'bone-like' reflectance: slope + broad sine + Gaussian absorption bands + detector steps + noise."""
    r = np.random.default_rng(seed)
    R = 0.32 + 0.05 * np.sin(wl / 97.0) + 4e-5 * (wl - 1500.0)
    for c, w, d in [(1450, 30, 0.06), (1690, 12, 0.012), (1730, 10, 0.015), (1930, 35, 0.08),
                    (2045, 15, 0.02), (2175, 14, 0.015), (2262, 12, 0.012), (2340, 18, 0.02)]:
        R -= d * np.exp(-0.5 * ((wl - c) / w) ** 2)
    R[wl <= joins[0]] += steps[0]
    R[wl > joins[1]] += steps[1]
    R += r.normal(0, noise, wl.size)
    return R


WL1 = np.arange(950.0, 2351.0)               # 1401 points, joins 1000/1800 inside
S1 = synth_bone_like(WL1)
S1[:3] = [0.0, -0.01, 5e-7]                  # values at/below the clips: the clip must be applied
WL2 = np.arange(1950.0, 2070.0)              # 120 points, no join
S2 = 0.4 + 0.1 * np.sin(WL2 / 7.0) + 0.02 * np.cos(WL2 / 2.3) + rng.normal(0, 0.003, WL2.size)
WL3 = np.arange(950.0, 2351.0)               # stress: steps at non-standard joins 1000/1830
S3 = synth_bone_like(WL3, steps=(0.02, -0.015), joins=(1000.0, 1830.0), seed=3)
SPECTRA = {"S1": (WL1, S1), "S2": (WL2, S2), "S3": (WL3, S3)}

cases = []


def case(name, spec, joins, chain, expect_error=None):
    wl, x = SPECTRA[spec]
    ctx = sr.ScanContext(joins)
    d = {"name": name, "spectrum": spec, "joins": list(joins), "chain": chain}
    if expect_error:
        try:
            sr.run_chain(chain, wl, x, ctx)
            ref_ok = True
        except sr.SpyderError:
            ref_ok = False
        d["expect_error"] = expect_error            # the Rust error kind
        d["spyder_ref_raises"] = not ref_ok          # documented: whole-grid SNV etc. are research-only in Python
    else:
        w, y = sr.run_chain(chain, wl, x, ctx)
        st = np.diff(w)
        if w.size > 1 and np.all(st == st[0]):
            d["expected_grid"] = {"first": float(w[0]), "step": float(st[0]), "n": int(w.size)}
        else:
            d["expected_wl"] = fl(w)
        d["expected"] = fl(y[0])
    cases.append(d)


AB6 = {"op": "absorbance", "clip_min": 1e-6}
AB3 = {"op": "absorbance", "clip_min": 1e-3}
J = (1000.0, 1800.0)
case("absorbance clip 1e-6", "S1", J, [AB6])
case("absorbance clip 1e-3", "S1", J, [AB3])
for (W, P, D, delta) in [(31, 3, 2, 1.0), (23, 2, 1, 1.0), (21, 3, 2, 2.0), (13, 4, 3, 1.0), (51, 5, 4, 1.0)]:
    for mode in ("interp", "mirror", "nearest", "constant", "wrap", "zero_edges", "valid"):
        case(f"savgol w{W} p{P} d{D} delta{delta} {mode}", "S2", J,
             [{"op": "savgol", "window": W, "polyorder": P, "deriv": D, "delta": delta, "mode": mode}])
SG31 = {"op": "savgol", "window": 31, "polyorder": 3, "deriv": 2, "delta": 1.0, "mode": "zero_edges"}
SG23 = {"op": "savgol", "window": 23, "polyorder": 2, "deriv": 1, "delta": 1.0, "mode": "zero_edges"}
case("Ryder chain: absorbance 1e-6, SG31 cubic d2 zero_edges", "S1", J, [AB6, SG31])
case("Ryder chain then crop 2030-2060", "S1", J, [AB6, SG31, {"op": "crop", "range_nm": [2030, 2060]}])
case("crop 1500-1550", "S1", J, [{"op": "crop", "range_nm": [1500.0, 1550.0]}])
BLUR = {"op": "gaussian_blur", "sigma_nm": [0.0, 3.3973, 3.3973], "truncate": 4.0, "radius": "ceil",
        "segments": "from_scan"}
case("blur v0.2 sigma per segment, ceil", "S1", J, [BLUR])
case("blur scalar sigma 2.5, round", "S1", J,
     [{"op": "gaussian_blur", "sigma_nm": 2.5, "truncate": 4.0, "radius": "round", "segments": "from_scan"}])
case("blur at non-standard joins 1000/1830 (stress)", "S3", (1000.0, 1830.0), [BLUR])
AFF2 = {"op": "absorbance_affine", "gain": [0.6343, 0.705, 0.6284], "offset": [0.2312, 0.2073, 0.2441],
        "clip_min": 1e-6}
case("absorbance_affine scalar", "S1", J, [{"op": "absorbance_affine", "gain": 0.6343, "offset": 0.2312,
                                             "clip_min": 1e-6}])
case("absorbance_affine per segment", "S1", J, [AFF2])
case("transfer v0.2 chain (blur then affine)", "S1", J, [BLUR, AFF2])
case("transfer v0.2 then Ryder chain", "S1", J, [BLUR, AFF2, AB6, SG31])
case("S1_R2-like: absorbance, SG23 d1, block SNV 1480-1650", "S1", J,
     [AB6, SG23, {"op": "snv", "ddof": 0, "blocks_nm": [[1480, 1650]]}])
case("block SNV two blocks", "S1", J, [AB6, SG23, {"op": "snv", "ddof": 0, "blocks_nm": [[1480, 1650], [1990, 2250]]}])
case("block SNV ddof 1", "S1", J, [AB6, SG23, {"op": "snv", "ddof": 1, "blocks_nm": [[1200, 1300]]}])
# must fail
case("block SNV spanning the 1800 nm join", "S1", J, [AB6, SG23, {"op": "snv", "ddof": 0, "blocks_nm": [[1700, 1900]]}],
     expect_error="failed")
case("whole-grid SNV is reserved", "S1", J, [AB6, {"op": "snv", "ddof": 0}], expect_error="unsupported_operator")
case("splice_correct is reserved", "S1", J, [{"op": "splice_correct", "joins": "from_scan"}],
     expect_error="unsupported_operator")
case("msc is reserved", "S2", J, [{"op": "msc", "reference": fl(S2)}], expect_error="unsupported_operator")
case("detrend is reserved", "S2", J, [{"op": "detrend", "degree": 1}], expect_error="unsupported_operator")
case("resample is reserved", "S2", J, [{"op": "resample", "to_grid": {"start_nm": 1960, "step_nm": 2, "n": 10}}],
     expect_error="unsupported_operator")
case("pds_banded is reserved", "S2", J, [{"op": "pds_banded", "wavelengths_nm": fl(WL2), "B": [[1.0]] * len(WL2)}],
     expect_error="unsupported_operator")
case("affine_reflectance is reserved", "S1", J,
     [{"op": "affine_reflectance", "segments": [{"a": 1, "b": 0}] * 3, "joins": "from_scan"}],
     expect_error="unsupported_operator")
case("unknown operator", "S2", J, [{"op": "wavelet"}], expect_error="unknown_operator")

# ------------------------------------------------------------------ SG weights
TABLE = [(31, 3, 2), (23, 3, 2), (23, 2, 1), (35, 2, 1), (41, 2, 1), (29, 2, 1), (27, 3, 2), (11, 2, 1),
         (13, 3, 2), (43, 3, 2), (33, 3, 2), (49, 3, 1), (23, 3, 1), (21, 3, 2), (37, 3, 2), (17, 3, 2),
         (25, 2, 1), (15, 3, 2), (29, 3, 2)]
WITH_EDGES = {(31, 3, 2), (35, 2, 1), (41, 2, 1), (29, 2, 1), (27, 3, 2), (11, 2, 1), (23, 3, 1)}
weights = []
for (W, P, d) in TABLE:
    for delta in (1.0, 2.0):
        h = W // 2
        e = {"window": W, "polyorder": P, "deriv": d, "delta": delta, "table": True,
             "centre": fl(sr.sg_centre_weights(W, P, d, delta))}
        if (W, P, d) in WITH_EDGES and delta == 1.0:
            e["left"] = sr._sg_matrix(W, P, d, delta, np.arange(h)).tolist()
            e["right"] = sr._sg_matrix(W, P, d, delta, np.arange(W - h, W)).tolist()
        weights.append(e)
for W in (5, 7, 15, 51):
    for P in range(0, 6):
        if P >= W:
            continue
        for d in range(0, min(P, 4) + 1):
            if (W, P, d) in TABLE:
                continue
            h = W // 2
            e = {"window": W, "polyorder": P, "deriv": d, "delta": 1.0, "table": False,
                 "centre": fl(sr.sg_centre_weights(W, P, d, 1.0))}
            if W == 7:
                e["left"] = sr._sg_matrix(W, P, d, 1.0, np.arange(h)).tolist()
                e["right"] = sr._sg_matrix(W, P, d, 1.0, np.arange(W - h, W)).tolist()
            weights.append(e)

# ------------------------------------------------------------------ numpy summation
sums = []
for n in [1, 2, 3, 7, 8, 9, 15, 16, 17, 31, 64, 100, 127, 128, 129, 171, 200, 257, 1000]:
    v = rng.normal(0.3, 1.0, n) * 10.0 ** rng.integers(-3, 4, n)
    sums.append({"n": n, "values": fl(v), "sum": float(np.sum(v)), "mean": float(np.mean(v)),
                 "std0": float(np.std(v)), "std1": float(np.std(v, ddof=1)) if n > 1 else None})

# ------------------------------------------------------------------ N2 on synthetic spectra (n2.py)
WLF = np.arange(1480.0, 2421.0)   # covers every N2 window; n2.py takes an explicit grid
n2_cases = []
for k, (noise, lvl) in enumerate([(1e-4, 0.3), (6e-4, 0.12), (3e-3, 0.05)]):
    A = -np.log10(lvl) + 1e-4 * (WLF - 350) + rng.normal(0, noise, WLF.size)
    R = 10 ** -A
    if k == 2:
        R[540:545] = -0.02          # non-positive values inside 2000-2100 (clipped at 1e-4)
        R[580] = np.nan             # a NaN inside 2000-2100 (dropped)
    n2_cases.append({"name": f"synthetic {k}", "wl": fl(WLF), "reflectance": [None if not np.isfinite(v) else float(v) for v in R],
                     "n2": {f"{lo}_{hi}": (None if not np.isfinite(n2_ref(R, lo, hi, wl=WLF)) else float(n2_ref(R, lo, hi, wl=WLF)))
                            for lo, hi in [(2000, 2100), (1500, 1600), (1500, 1550), (2300, 2400)]}})

doc = {
    "format": "spyder-bone/operator-goldens",
    "version": 1,
    "generated_by": "crates/spyder-core/tests/gen/gen_operator_goldens.py",
    "spyder_ref_sha256": hashlib.sha256(ref_dir.read_bytes()).hexdigest(),
    "n2_py_sha256": hashlib.sha256((N2_DIR / "n2.py").read_bytes()).hexdigest(),
    "numpy": np.__version__,
    "inputs": "synthetic only (analytic shapes + seeded noise); no measured spectra",
    "spectra": {k: {"wl": fl(w), "values": fl(x)} for k, (w, x) in SPECTRA.items()},
    "cases": cases,
    "sg_weights": weights,
    "np_sum": sums,
    "n2": n2_cases,
}
OUT.parent.mkdir(parents=True, exist_ok=True)
OUT.write_text(json.dumps(doc, separators=(",", ":")), encoding="utf-8", newline="\n")
print(f"wrote {OUT} ({OUT.stat().st_size / 1e3:.0f} kB): {len(cases)} cases, {len(weights)} weight sets, "
      f"{len(sums)} sum cases, {len(n2_cases)} N2 cases; spyder_ref from {ref_dir}")
