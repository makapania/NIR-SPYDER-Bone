"""Shared-table plug-ins (Phase 0b; format 2 since DECISIONS 137): bands.json, noise_gains.json, the instrument registry
and the catalog.

Loading rules the app must reproduce, and the golden runners for the two tables that compute numbers:
  bands       (format "spyder-bone/bands", format_version 2):        band readings r_b, and the readability SD_E of a
              band = its gain x envelope factor x N2(noise window), the gain COMPUTED (gains.py) from the band kernel,
              the correction applied to the stream and the scan's own detector segments
  noise_gains (format "spyder-bone/noise_gains", format_version 2):  the noise model, the N2 windows a model kernel's
              detector parts use, per-model settings (B6 policy, a recorded gain for a non-linear model) and the
              consensus factor; every linear model's implied SD is COMPUTED the same way
Nothing is keyed by a transfer file's SHA-256 any more (format 1 kept per-transfer gains). A missing or non-finite band
SD is +infinity: the band is unreadable unless strong (fail safe). Numbers compare with the engine tolerance
(1e-9 abs + 1e-9 rel).
"""
from __future__ import annotations
import math
from pathlib import Path
import numpy as np
from . import core as SR
from . import gains as G
from .n2 import n2

BANDS_FORMAT = "spyder-bone/bands"
NOISE_FORMAT = "spyder-bone/noise_gains"
INSTRUMENT_FORMAT = "spyder-bone/instrument"
CATALOG_FORMAT = "spyder-bone/catalog"
TABLE_FORMAT_VERSION = 2          # bands and noise gains (format 1 held per-transfer gains keyed by file sha256)
TOL = {"abs": 1e-9, "rel": 1e-9}
INF = float("inf")


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


NOISE_UNKNOWN = "not assessed: noise unknown (N2 not finite)"


def readability_sd(bd, band, wl, joins, ops, inoise, n2_of):
    """(SD_E, status) of a band reading on a stream made with this correction (ops None = none). status 'assessed',
    or why the noise is unknown; an unknown or non-finite SD is +infinity (unreadable unless strong: fail safe).
    (None, None) for a band without a readability block."""
    if bd["bands"][band].get("readability") is None:
        return None, None
    try:
        (lo, hi), g = G.band_gain(bd, band, wl, joins, ops, inoise)
    except G.GainError as e:
        return INF, e.status
    v = n2_of(lo, hi)
    if v is None or not math.isfinite(v) or not math.isfinite(g * v):
        return INF, NOISE_UNKNOWN
    return g * v, "assessed"


B6_NOT_APPLIED = "not assessed: B6 not applied to this reader"


def implied_sd(ng, m, wl, joins, ops=None, inoise=None, n2_of=None):
    """Implied SD (wt%) of a model reading on the AS-MEASURED scan through a correction: {'sd': float | None,
    'component_sd': {name: float | None} (consensus only, else None), 'status': 'assessed' | why not, 'b6': bool}. A
    consensus SD = factor x median of its components' SDs (each component folded from the consensus file itself)."""
    mid = m["id"]
    b6 = (ng.get("models", {}).get(mid) or {}).get("b6", True) is not False
    cons = m.get("kind") == "consensus"
    names = [c["name"] for c in m["consensus"]["components"]] if cons else []
    out = {"sd": None, "component_sd": {n: None for n in names} if cons else None, "status": None, "b6": b6}
    try:
        terms = G.model_terms(ng, m, wl, joins, ops, inoise)
    except G.GainError as e:
        out["status"] = e.status
        return out
    sds = {k: G.rss(t, n2_of) for k, t in terms.items()}
    if cons:
        out["component_sd"] = {n: sds.get(n) for n in names}
        f = (ng.get("consensus", {}).get(mid) or {}).get("factor")
        if f is None:
            out["status"] = "not assessed: no noise factor for this consensus"
        elif any(v is None for v in out["component_sd"].values()):
            out["status"] = NOISE_UNKNOWN
        else:
            out["sd"] = float(f) * float(np.median(list(out["component_sd"].values())))
            out["status"] = "assessed"
        return out
    sd = sds[None]
    out["sd"], out["status"] = (sd, "assessed") if sd is not None else (None, NOISE_UNKNOWN)
    return out


def _correction(golden, name):
    if name in (None, "none"):
        return None, {}
    c = (golden.get("corrections") or {}).get(name)
    if c is None:
        raise SR.SpyderError(f"golden: unknown correction {name!r}")
    return list(c["operator"]), dict(c.get("instrument_noise") or {})


def golden_models(doc, folder):
    """{model id: model doc} for a noise-gains golden block's `models` (id -> file sha256), found in the folder."""
    want = (doc.get("golden") or {}).get("models") or {}
    by_sha = {SR.file_sha256(p): p for p in sorted(Path(folder).glob("*.spyder-model.json"))}
    out = {}
    for mid, sha in want.items():
        p = by_sha.get(sha)
        if p is None:
            raise SR.SpyderError(f"golden: model {mid} (sha256 {sha[:12]}) is not in this folder")
        d = SR.load_json(p)
        if d.get("id") != mid:
            raise SR.SpyderError(f"golden: the file with sha256 {sha[:12]} is {d.get('id')}, not {mid}")
        out[mid] = d
    return out


def run_table_goldens(doc, golden_spectra, folder):
    """Golden cases of bands.json / noise_gains.json. Each case: spectrum_sha256, correction ('none' or a name in the
    golden block's `corrections`: an operator list and optional instrument_noise), optional splices_nm (default
    1000/1800), expected {name: value} with names 'E:<band>', 'SD_E:<band>' (bands) or 'N2:<lo>-<hi>', 'SD:<model id>'
    (noise gains; the models by file sha256 in the golden block's `models`)."""
    fmt = doc.get("format")
    tol = table_tolerance(doc)
    golden = doc.get("golden", {})
    cases = golden.get("cases", [])
    if len(cases) < SR.GOLDEN_MIN_CASES:
        raise SR.SpyderError(f"golden: at least {SR.GOLDEN_MIN_CASES} cases required")
    models = golden_models(doc, folder) if fmt == NOISE_FORMAT else {}
    res = []
    covered = set()
    for c in cases:
        if c["spectrum_sha256"] not in golden_spectra:
            raise SR.SpyderError(f"golden case {c.get('name')}: spectrum not in the golden spectra file")
        wl, x, _ = golden_spectra[c["spectrum_sha256"]]
        joins = tuple(float(j) for j in c.get("splices_nm", (1000.0, 1800.0)))
        ops, inn = _correction(golden, c.get("correction", "none"))
        if not c.get("expected"):
            raise SR.SpyderError(f"golden case {c.get('name')}: empty expected block")
        n2_of = lambda lo, hi, x=x, wl=wl: n2(x, lo, hi, wl=wl)
        if fmt == BANDS_FORMAT:
            st = np.atleast_2d(x) if ops is None else SR.run_chain(ops, wl, np.atleast_2d(x), SR.ScanContext(joins))[1]
            E = band_E(doc, st, wl)
        for name, want in c["expected"].items():
            kind, _, what = name.partition(":")
            if fmt == BANDS_FORMAT and kind == "E":
                if what not in doc["bands"]:
                    raise SR.SpyderError(f"golden case {c.get('name')}: unknown band {what}")
                got = float(E[what][0])
            elif fmt == BANDS_FORMAT and kind == "SD_E":
                if what not in doc["bands"]:
                    raise SR.SpyderError(f"golden case {c.get('name')}: unknown band {what}")
                got = readability_sd(doc, what, wl, joins, ops, inn, n2_of)[0]
                covered.add(what)
            elif fmt == NOISE_FORMAT and kind == "N2":
                lo, hi = (float(v) for v in what.split("-"))
                got = n2_of(lo, hi)
            elif fmt == NOISE_FORMAT and kind == "SD":
                if what not in models:
                    raise SR.SpyderError(f"golden case {c.get('name')}: model {what} is not in the golden block's models")
                got = implied_sd(doc, models[what], wl, joins, ops, inn, n2_of)["sd"]
                covered.add(what)
            else:
                raise SR.SpyderError(f"golden case {c.get('name')}: unknown expected output {name}")
            res.append((c.get("name"), name, got, want, got is not None and _ok(float(got), float(want), tol)))
    if not res:
        raise SR.SpyderError("golden: zero comparisons")
    # the goldens must exercise every computed output the table governs (Codex Phase 2 MEDIUM 6)
    if fmt == BANDS_FORMAT:
        need = {b for b, e in doc["bands"].items() if e.get("readability") is not None}
    else:
        need = set(models) | set(doc.get("consensus", {})) | {k for k, v in doc.get("models", {}).items() if "recorded" in v}
    missing = sorted(need - covered)
    if missing:
        raise SR.SpyderError(f"golden: no case tests {', '.join(missing)}")
    return res


def _pos(x):
    return isinstance(x, (int, float)) and not isinstance(x, bool) and math.isfinite(x) and x > 0


def _check_window(w, what):
    if not (isinstance(w, list) and len(w) == 2 and all(isinstance(v, (int, float)) and not isinstance(v, bool)
                                                        and math.isfinite(v) for v in w) and w[0] < w[1]):
        raise SR.SpyderError(f"{what}: a window must be [lo, hi] with lo < hi")


def _check_noise_model(doc):
    nm = doc.get("noise_model")
    if not isinstance(nm, dict):
        raise SR.SpyderError("noise_model block required")
    v = nm.get("correlation_nm")
    if not (isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v) and v >= 0):
        raise SR.SpyderError("noise_model.correlation_nm must be a finite number >= 0")


def check_bands(doc):
    if doc.get("format") != BANDS_FORMAT or doc.get("format_version") != TABLE_FORMAT_VERSION:
        raise SR.SpyderError(f"not a bands table of format_version {TABLE_FORMAT_VERSION}")
    _check_noise_model(doc)
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
            _check_window(r.get("noise_window_nm"), f"band {b}")
            if "gain_E_per_N2" in r:
                raise SR.SpyderError(f"band {b}: per-transfer gains are format 1 (format 2 computes them)")
            if "envelope_factor" in r and not _pos(r["envelope_factor"]):
                raise SR.SpyderError(f"band {b}: envelope_factor must be a positive finite number")
    return doc


def check_noise_gains(doc):
    if doc.get("format") != NOISE_FORMAT or doc.get("format_version") != TABLE_FORMAT_VERSION:
        raise SR.SpyderError(f"not a noise-gains table of format_version {TABLE_FORMAT_VERSION}")
    _check_noise_model(doc)
    ws = doc.get("model_noise_windows_nm")
    if not isinstance(ws, list) or not ws:
        raise SR.SpyderError("model_noise_windows_nm: at least one window")
    for w in ws:
        _check_window(w, "model_noise_windows_nm")
    for mid, m in doc.get("models", {}).items():
        if "b6" in m and not isinstance(m["b6"], bool):
            raise SR.SpyderError(f"{mid}: b6 must be true or false")
        rec = m.get("recorded")
        if rec is not None:
            if not rec.get("terms"):
                raise SR.SpyderError(f"{mid}: a recorded gain needs at least one term")
            for t in rec["terms"]:
                _check_window(t.get("window_nm"), f"{mid}")
                if not _pos(t.get("gain")):
                    raise SR.SpyderError(f"{mid}: bad recorded term {t}")
    for mid, c in doc.get("consensus", {}).items():
        if not _pos(c.get("factor")):
            raise SR.SpyderError(f"consensus {mid}: bad factor")
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
