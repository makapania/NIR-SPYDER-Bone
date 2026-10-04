"""Shared-table plug-ins (Phase 0b): bands.json, noise_gains.json, the instrument registry and the catalog.

Loading rules the app must reproduce, and the golden runners for the two tables that compute numbers:
  bands       (format "spyder-bone/bands"):        band readings r_b and readability SD_E per transfer key
  noise_gains (format "spyder-bone/noise_gains"):   implied SD of each collagen model per transfer key
Transfer key = "none" or the sha256 of the transfer FILE applied before the reading/model; a key with no entry means
"not assessed" (never a guess). Numbers compare with the engine tolerance (1e-9 abs + 1e-9 rel).
"""
from __future__ import annotations
import math
from pathlib import Path
import numpy as np
from . import core as SR
from .n2 import n2

BANDS_FORMAT = "spyder-bone/bands"
NOISE_FORMAT = "spyder-bone/noise_gains"
INSTRUMENT_FORMAT = "spyder-bone/instrument"
CATALOG_FORMAT = "spyder-bone/catalog"
TOL = {"abs": 1e-9, "rel": 1e-9}


def _ok(got, want, tol=TOL):
    return math.isfinite(got) and abs(got - want) <= tol["abs"] + tol["rel"] * abs(want)


def table_tolerance(doc):
    """The tolerance a table's goldens are checked at: the engine maximum (TOL) unless the file states a TIGHTER
    one; a looser or malformed one rejects the file (as core.golden_tolerance does for models and transfers)."""
    tol = doc.get("golden", {}).get("tolerance", TOL)
    out = {}
    for k in ("abs", "rel"):
        v = tol.get(k) if isinstance(tol, dict) else None
        if isinstance(v, bool) or not isinstance(v, (int, float)) or not math.isfinite(v) or v < 0:
            raise SR.SpyderError(f"golden tolerance '{k}' must be a finite number >= 0")
        if v > TOL[k]:
            raise SR.SpyderError(f"golden tolerance {k}={v:g} is looser than the engine maximum {TOL[k]:g}")
        out[k] = float(v)
    return out


def transfers_by_sha(folder):
    """{file sha256: transfer doc} for every *.spyder-transfer.json in the folder (non-recursive)."""
    out = {}
    for p in sorted(Path(folder).glob("*.spyder-transfer.json")):
        out[SR.file_sha256(p)] = SR.check_transfer(SR.load_json(p))
    return out


def stream(R_raw, key, transfers, wl):
    """The stream a reading is computed on: the scan itself for key 'none', else the keyed transfer applied."""
    if key == "none":
        return np.atleast_2d(R_raw)
    t = transfers.get(key)
    if t is None:
        raise SR.SpyderError(f"transfer key {key[:12]} has no transfer file in this folder")
    return SR.apply_transfer(t, wl, np.atleast_2d(R_raw), SR.ScanContext((1000.0, 1800.0), t["source_class"]))[1]


def projected_E(doc, name, E, wl):
    """E with a bands.json projection applied over its window (NaN elsewhere): x = E / scale, x' = x - v (v.(x - c)),
    E' = scale x'. The projection's kernel must be the band's kernel (checked by check_bands)."""
    pj = doc["projections"][name]
    sc = doc["kernels"][pj["kernel"]]["scale"]
    lo, hi = pj["window_nm"]
    m = (wl >= lo - 1e-9) & (wl <= hi + 1e-9)
    v = np.asarray(pj["direction"], float); c = np.asarray(pj["centre"], float)
    X = np.atleast_2d(E)[:, m] / sc
    out = np.full_like(np.atleast_2d(E), np.nan)
    out[:, m] = sc * (X - np.outer((X - c) @ v, v))
    return out


def band_E(doc, R, wl):
    """{band: r_b} on the given stream; kernel from the bands table."""
    out = {}
    cache = {}
    for b, e in doc["bands"].items():
        k = doc["kernels"][e["kernel"]]
        if e["kernel"] not in cache:
            ops = [{"op": "absorbance", "clip_min": k["absorbance_clip_min"]},
                   dict({"op": "savgol"}, **k["savgol"])]
            cache[e["kernel"]] = (SR.run_chain(ops, wl, np.atleast_2d(R), SR.ScanContext())[1] * k["scale"])
        E = cache[e["kernel"]]
        if e.get("projection"):
            E = projected_E(doc, e["projection"], E, wl)
        c, h = e["centre_nm"], e["half_width_nm"]
        m = (wl >= c - h - 1e-9) & (wl <= c + h + 1e-9)
        out[b] = E[:, m].mean(1)
    return out


def readability_sd(doc, band, R_raw, key):
    r = doc["bands"][band].get("readability")
    if r is None:
        return None
    g = r["gain_E_per_N2"].get(key)
    if g is None:
        return None                                # not assessed: no gain for this transfer
    lo, hi = r["noise_window_nm"]
    return g * n2(R_raw, lo, hi)


def implied_sd(doc, model_id, R_raw, key):
    """Implied SD (wt%) of a collagen model on the as-measured scan R_raw for transfer key; None = not assessed."""
    if model_id in doc.get("consensus", {}):
        c = doc["consensus"][model_id]
        comp = [implied_sd(doc, mid, R_raw, key) for mid in c["components"].values()]
        if any(x is None for x in comp):
            return None
        return c["factor"] * float(np.median(comp))
    m = doc["models"].get(model_id)
    if m is None or key not in m:
        return None
    return float(math.sqrt(sum((t["gain"] * n2(R_raw, *t["window_nm"])) ** 2 for t in m[key]["terms"])))


def run_table_goldens(doc, golden_spectra, folder):
    """Golden cases of bands.json / noise_gains.json. Each case: spectrum_sha256, transfer_key, expected {name: value}
    with names 'E:<band>', 'SD_E:<band>' (bands) or 'N2:<lo>-<hi>', 'SD:<model id>' (noise gains)."""
    fmt = doc.get("format")
    tol = table_tolerance(doc)
    cases = doc.get("golden", {}).get("cases", [])
    if len(cases) < SR.GOLDEN_MIN_CASES:
        raise SR.SpyderError(f"golden: at least {SR.GOLDEN_MIN_CASES} cases required")
    transfers = transfers_by_sha(folder)
    res = []
    for c in cases:
        if c["spectrum_sha256"] not in golden_spectra:
            raise SR.SpyderError(f"golden case {c.get('name')}: spectrum not in the golden spectra file")
        wl, x, _ = golden_spectra[c["spectrum_sha256"]]
        key = c["transfer_key"]
        if not c.get("expected"):
            raise SR.SpyderError(f"golden case {c.get('name')}: empty expected block")
        if fmt == BANDS_FORMAT:
            E = band_E(doc, stream(x, key, transfers, wl), wl)
        for name, want in c["expected"].items():
            kind, _, what = name.partition(":")
            if fmt == BANDS_FORMAT and kind == "E":
                got = float(E[what][0])
            elif fmt == BANDS_FORMAT and kind == "SD_E":
                got = readability_sd(doc, what, x, key)
            elif fmt == NOISE_FORMAT and kind == "N2":
                lo, hi = (float(v) for v in what.split("-"))
                got = n2(x, lo, hi)
            elif fmt == NOISE_FORMAT and kind == "SD":
                got = implied_sd(doc, what, x, key)
            else:
                raise SR.SpyderError(f"golden case {c.get('name')}: unknown expected output {name}")
            res.append((c.get("name"), name, got, want, got is not None and _ok(float(got), float(want), tol)))
    if not res:
        raise SR.SpyderError("golden: zero comparisons")
    return res


def check_bands(doc):
    if doc.get("format") != BANDS_FORMAT or doc.get("format_version") != 1:
        raise SR.SpyderError("not a bands table")
    for b, e in doc["bands"].items():
        if e["kernel"] not in doc["kernels"]:
            raise SR.SpyderError(f"band {b}: unknown kernel {e['kernel']}")
        if e.get("projection") is not None:
            pj = doc.get("projections", {}).get(e["projection"])
            if pj is None:
                raise SR.SpyderError(f"band {b}: unknown projection {e['projection']}")
            lo, hi = pj["window_nm"]
            n = int(round(hi - lo)) + 1
            if pj["kernel"] != e["kernel"] or len(pj["direction"]) != n or len(pj["centre"]) != n:
                raise SR.SpyderError(f"band {b}: projection {e['projection']} does not match its window or kernel")
            if not (lo <= e["centre_nm"] - e["half_width_nm"] and e["centre_nm"] + e["half_width_nm"] <= hi):
                raise SR.SpyderError(f"band {b}: read window outside the projection window")
        if e.get("u_sign", 1) not in (-1, 1):
            raise SR.SpyderError(f"band {b}: u_sign must be -1 or 1")
        r = e.get("readability")
        if r is not None:
            lo, hi = r["noise_window_nm"]
            if not lo < hi:
                raise SR.SpyderError(f"band {b}: bad noise window")
            for k, g in r["gain_E_per_N2"].items():
                if not (isinstance(g, (int, float)) and math.isfinite(g) and g > 0):
                    raise SR.SpyderError(f"band {b}: gain for {k} must be a positive finite number")
    return doc


def check_noise_gains(doc):
    if doc.get("format") != NOISE_FORMAT or doc.get("format_version") != 1:
        raise SR.SpyderError("not a noise-gains table")
    for mid, m in doc["models"].items():
        for k, e in m.items():
            for t in e["terms"]:
                if not (t["gain"] > 0 and math.isfinite(t["gain"]) and t["window_nm"][0] < t["window_nm"][1]):
                    raise SR.SpyderError(f"{mid} [{k}]: bad term {t}")
    for mid, c in doc.get("consensus", {}).items():
        if not c["factor"] > 0 or len(c["components"]) % 2 != 1:
            raise SR.SpyderError(f"consensus {mid}: bad factor or component count")
        for comp in c["components"].values():
            if comp not in doc["models"]:
                raise SR.SpyderError(f"consensus {mid}: component {comp} has no gains")
    return doc


def check_instruments(doc):
    if doc.get("format") != INSTRUMENT_FORMAT or doc.get("format_version") != 1:
        raise SR.SpyderError("not an instrument registry")
    seen = {}
    for cls, v in doc["classes"].items():
        for s in v.get("serials", []):
            if s in seen:
                raise SR.SpyderError(f"serial {s} listed for two classes")
            seen[s] = cls
    if doc["default_class"] not in doc["classes"]:
        raise SR.SpyderError("default_class is not a listed class")
    return doc


def validate_table(path, golden_spectra=None):
    """(n_checks, n_failed, failures, reason) like core.validate_file, for bands / noise_gains / instrument files."""
    try:
        doc = SR.load_json(path)
        fmt = doc.get("format")
        if fmt == INSTRUMENT_FORMAT:
            check_instruments(doc)
            return 1, 0, [], None
        (check_bands if fmt == BANDS_FORMAT else check_noise_gains)(doc)
        if golden_spectra is None:
            golden_spectra = SR.load_golden_spectra(SR.package_path(Path(path).parent, doc["golden"]["spectra_file"],
                                                                    "golden spectra file"))
        r = run_table_goldens(doc, golden_spectra, Path(path).parent)
    except (SR.SpyderError, KeyError, TypeError, ValueError) as e:
        return 0, 1, [], f"{type(e).__name__}: {e}"
    bad = [x for x in r if not x[-1]]
    return len(r), len(bad), bad, None
