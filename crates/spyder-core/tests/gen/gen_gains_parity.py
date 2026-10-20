"""Write crates/spyder-core/tests/goldens/gains_parity_v1.json: the frozen Python reference's computed noise and
band-readability gains (reference/spyder_ref/gains.py, DECISIONS 137) for every shipped model and readable band,
through a set of corrections (none, the shipped v0.1 / v0.2 / v0.3 files, v0.2 with an instrument's own noise, a composed
linear file, a non-linear file, a file whose per-segment values do not fit the scan, white noise), on several scan
layouts (the LabSpec joins and others, one join, no join, a 900-1700 nm grid, a 1598-1750 nm grid that holds the
C-H 1689 kernel but only part of its noise window, a 0.5 nm grid). Also the checks' linear statistics (the
contaminant-sign statistics, the C1 excess and its variants, the long-wave tail reading; DECISIONS 137 g2), one detector
term per N2 window, defined in the fixture itself from check.signs / check.c1. Errors are recorded by
their status text. The Rust test tests/gains_parity.rs recomputes every entry and requires gains within 1e-9 relative
and identical status texts. Public files only.

    python crates/spyder-core/tests/gen/gen_gains_parity.py
"""
import json
import sys
from pathlib import Path

import numpy as np

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
from parity_common import REPO, SR  # noqa: E402
from spyder_ref import gains as G  # noqa: E402

PLUGINS = REPO / "plugins"
OUT = REPO / "crates" / "spyder-core" / "tests" / "goldens" / "gains_parity_v1.json"


def strip(ops):
    return [{k: v for k, v in op.items() if k != "note"} for op in ops]


T02 = SR.load_json(PLUGINS / "hires_to_std_v0_2.spyder-transfer.json")
T01 = SR.load_json(PLUGINS / "hires_to_std_v0_1.spyder-transfer.json")
T03 = SR.load_json(PLUGINS / "hires_to_std_v0_3.spyder-transfer.json")
CORRECTIONS = {
    "none": None,
    "v0.1": {"operator": strip(T01["operator"])},
    "v0.2": {"operator": strip(T02["operator"])},
    "v0.3": {"operator": strip(T03["operator"])},
    "v0.2_lab_noise": {"operator": strip(T02["operator"]),
                       "instrument_noise": {"correlation_nm": 1.6,
                                            "ch_envelope_factors": {"CH1689": 1.3, "CH2284": 1.1}}},
    "composed": {"operator": [
        {"op": "gaussian_blur", "sigma_nm": [0.0, 2.0, 2.5], "truncate": 4.0, "radius": "ceil", "segments": "from_scan"},
        {"op": "absorbance_affine", "gain": [0.9, 0.8, 0.7], "offset": [0.1, 0.2, 0.3], "clip_min": 1e-6},
        {"op": "gaussian_blur", "sigma_nm": 1.5, "truncate": 3.0, "radius": "round", "segments": "from_scan"},
        {"op": "absorbance_affine", "gain": 1.1, "clip_min": 1e-6}]},
    "white_noise": {"operator": strip(T02["operator"]), "instrument_noise": {"correlation_nm": 0.0}},
    "nonlinear": {"operator": strip(T02["operator"]) + [{"op": "snv", "ddof": 0, "blocks_nm": [[1100.0, 1700.0]]}]},
    "two_segment_values": {"operator": [
        {"op": "gaussian_blur", "sigma_nm": [0.0, 3.0], "truncate": 4.0, "radius": "ceil", "segments": "from_scan"}]},
}
STD = np.arange(350, 2501, dtype=float)
LAYOUTS = [
    ("std 1000/1800", STD, [1000.0, 1800.0]),
    ("std 1000/1785", STD, [1000.0, 1785.0]),
    ("std 1000/1765", STD, [1000.0, 1765.0]),
    ("std 1000/1740", STD, [1000.0, 1740.0]),
    ("std 980/1830", STD, [980.0, 1830.0]),
    ("std one join 1000", STD, [1000.0]),
    ("std no join", STD, []),
    ("900-1700 no join", np.arange(900, 1701, dtype=float), []),
    ("1598-1750 no join", np.arange(1598, 1751, dtype=float), []),
    ("0.5 nm 1000/1800", np.arange(700, 4601, dtype=float) / 2.0, [1000.0, 1800.0]),
]


def kernel(k):
    return {"absorbance_clip": k["absorbance_clip"], "savgol": dict(k["savgol"]), "scale": k["scale"]}


def sharp(c, h, f0, f1, w=1.0):
    return [[c - h, c + h, w], [c - f1, c - f0, -0.5 * w], [c + f0, c + f1, -0.5 * w]]


def c1_windows(q, hw):
    return [[c - hw, c + hw, 1.0] for c in q["ch_bands_nm"]] + [[c - hw, c + hw, -q["b"]] for c in q["nh_bands_nm"]]


def statistics():
    """The checks' linear statistics (sum of weight x mean E over [lo, hi]) and the N2 windows each is read in."""
    sg = SR.load_json(PLUGINS / "checks" / "check.signs.json")["parameters"]
    c1 = SR.load_json(PLUGINS / "checks" / "check.c1.json")["parameters"]
    ks, kc = kernel(sg["kernel"]), kernel(c1["kernel"])
    pb = sg["plaster"]["bands"]
    hw = c1["read_half_width_nm"]
    W1, W2, WE, WT = [1500.0, 1600.0], [2000.0, 2100.0], [2220.0, 2245.0], [2300.0, 2400.0]
    return {
        "plaster_1446": ({"kernel": ks, "windows": sharp(*pb[0]["sharp"], 1.0 / pb[0]["plaster_contrast"])}, [W1]),
        "plaster_1945": ({"kernel": ks, "windows": sharp(*pb[1]["sharp"], 1.0 / pb[1]["plaster_contrast"])}, [W2]),
        "wax_1766": ({"kernel": ks, "windows": sharp(*sg["wax"]["all_of"][0]["sharp"])}, [W1]),
        "ester_2254": ({"kernel": ks, "windows": sharp(*sg["ester"]["sharp"])}, [WE, W2]),
        "c1": ({"kernel": kc, "windows": c1_windows(c1, hw)}, [W1, W2]),
        "c1_swir1": ({"kernel": kc, "windows": c1_windows(c1["swir1"], hw)}, [W1, W2]),
        "tail_2350": ({"kernel": kc, "windows": [[2348.0, 2352.0, 1.0]]}, [WT]),
    }


def models():
    out = []
    for p in sorted(PLUGINS.glob("*.spyder-model.json")):
        out.append((p.name, SR.load_json(p)))
    return out


def main():
    global STATS
    STATS = statistics()
    ng = SR.load_json(PLUGINS / "noise_gains.json")
    bd = SR.load_json(PLUGINS / "bands.json")
    readable = [b for b, e in bd["bands"].items() if e.get("readability")]
    entries = []
    for lname, wl, joins in LAYOUTS:
        for cname, c in CORRECTIONS.items():
            if lname.startswith("0.5 nm") and cname not in ("none", "v0.2", "v0.3"):
                continue
            ops, inn = (None, {}) if c is None else (c["operator"], c.get("instrument_noise", {}))
            for fname, m in models():
                try:
                    t = G.model_terms(ng, m, wl, joins, ops, inn)
                    got = {("" if k is None else k): [[w[0], w[1], g] for w, g in v] for k, v in t.items()}
                except G.GainError as e:
                    got = {"error": e.status}
                entries.append({"layout": lname, "correction": cname, "model": fname, "terms": got})
            for b in readable:
                try:
                    (lo, hi), g = G.band_gain(bd, b, wl, joins, ops, inn)
                    got = {"window": [lo, hi], "gain": g}
                except G.GainError as e:
                    got = {"error": e.status}
                entries.append({"layout": lname, "correction": cname, "band": b, "gain": got})
            for sname, (st, wins) in STATS.items():
                for w in wins:
                    try:
                        got = {"gain": G.stat_term_gain(st, wl, joins, ops, inn, 1.25, tuple(w), sname)}
                    except G.GainError as e:
                        got = {"error": e.status}
                    entries.append({"layout": lname, "correction": cname, "statistic": sname, "window": w,
                                    "result": got})
    files = {p.name: SR.file_sha256(p) for p in [PLUGINS / "noise_gains.json", PLUGINS / "bands.json"]
             + sorted(PLUGINS.glob("*.spyder-model.json"))}
    doc = {"format": "spyder-bone/gains-parity", "version": 1,
           "generated_by": "crates/spyder-core/tests/gen/gen_gains_parity.py (reference/spyder_ref/gains.py)",
           "plugin_files": files,
           "layouts": [{"name": n, "wl_start": float(w[0]), "wl_step": float(w[1] - w[0]), "n": len(w), "joins": j}
                       for n, w, j in LAYOUTS],
           "corrections": CORRECTIONS, "statistics": {k: v[0] for k, v in STATS.items()}, "entries": entries}
    OUT.write_bytes((json.dumps(doc, separators=(",", ":"), allow_nan=False) + "\n").encode("utf-8"))
    n_err = sum(1 for e in entries if "error" in e.get("terms", {}) or "error" in e.get("gain", {})
                or "error" in e.get("result", {}))
    print(f"wrote {OUT.relative_to(REPO)}: {len(entries)} entries ({n_err} with a status instead of a gain)")


if __name__ == "__main__":
    main()
