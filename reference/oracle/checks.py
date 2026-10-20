"""Engine checks (PLAN v3.2 Steps 2, 2c, 5, 7, 8, 9). Each function takes ONE scan and the parameters loaded from
its check file, and returns plain dicts. Every check reports a status separately from its result:
  "assessed" | "not assessed: <reason>" | "gated: <reason>"  (a skipped check never reads as "not detected").
"""
from __future__ import annotations

import math

import numpy as np

from . import kernels as K
from .n2 import n2 as _n2

NAN = float("nan")
INF = float("inf")


def _band_sd(sd_E, b):
    """A band's noise SD for the readability gate: a missing or NaN SD is +infinity (fail safe: unknown noise is "too
    noisy", never "readable"; such a band is can't tell unless strong)."""
    v = sd_E.get(b)
    return INF if v is None or math.isnan(v) else v


def _f(x):
    x = float(x)
    return x if math.isfinite(x) else None


# ====================================================================== Step 2: acquisition checks (as measured)
class N2Cache:
    def __init__(self, wl, R_meas):
        self.wl, self.R = np.asarray(wl, float), np.asarray(R_meas, float)
        self.v = {}

    def __call__(self, lo, hi):
        k = (float(lo), float(hi))
        if k not in self.v:
            self.v[k] = _n2(self.R, k[0], k[1], wl=self.wl)
        return self.v[k]

    def table(self):
        return {f"{int(a)}-{int(b)}": _f(v) for (a, b), v in sorted(self.v.items())}


def acquisition(wl, R, P, dn=None, joins=(1000.0, 1800.0), no_dn_status="not assessed: no raw DN (spectrum input)"):
    """B2-B5, B7, B8 on the as-measured reflectance (B1 is the reader's matrix; B6/B6b are in pipeline). B7 and B8 sit
    on the scan's own joins (DECISIONS 87)."""
    wl = np.asarray(wl, float); R = np.asarray(R, float)
    out = {}
    lo, hi = P["integrity_range_nm"]
    k = (wl >= lo) & (wl <= hi)
    # B2 saturation (needs raw DN)
    b2 = P["B2_saturation"]
    if dn is None:
        out["B2"] = {"status": no_dn_status, "outcome": None}
    else:
        m = float(np.nanmax(np.asarray(dn, float)[k]))
        out["B2"] = {"status": "assessed", "max_dn": m, "outcome": "Unusable" if m >= b2["dn_at_least"] else "ok"}
    # B3 impossible reflectance
    bad = bool(np.any(~np.isfinite(R[k])) or np.any(R[k] <= P["B3_impossible"]["r_at_most"]))
    out["B3"] = {"status": "assessed", "outcome": "Unusable" if bad else "ok"}
    # B4 panel or empty probe (reference saves are listed by the reader, not scored)
    b4 = P["B4_panel_or_empty"]
    mr, sd = float(np.mean(R[k])), float(np.std(R[k]))
    panel = b4["panel_mean_range"][0] <= mr <= b4["panel_mean_range"][1] and sd < b4["panel_sd_below"]
    empty = mr < b4["empty_mean_below"]
    out["B4"] = {"status": "assessed", "mean_R": _f(mr), "sd_R": _f(sd),
                 "outcome": "Unusable" if (panel or empty) else "ok",
                 "reason": "white panel" if panel else ("empty probe" if empty else None)}
    # B5 very dark
    b5 = P["B5_dark"]
    out["B5"] = {"status": "assessed", "mean_R": _f(mr), "outcome": "Note" if mr < b5["mean_R_below"] else "ok"}
    # B7 reflectance above 1
    b7 = P["B7_above_one"]
    j1, j2 = sorted(float(x) for x in joins)
    k7 = (wl >= j1 + b7["range_offsets_nm"][0]) & (wl <= j2 + b7["range_offsets_nm"][1])
    mx = float(np.max(R[k7]))
    out["B7"] = {"status": "assessed", "max_R": _f(mx), "outcome": "Note" if mx > b7["max_R_above"] else "ok"}
    # B8 splice steps at the header joins (both line fits on the SWIR1 side)
    b8 = P["B8_splice_step"]
    steps = []
    js = sorted(float(x) for x in joins)
    for j in b8["fits"]:
        jn = js[j["join_index"]]
        a, b = K.idx(wl, jn + j["fit_offsets_nm"][0]), K.idx(wl, jn + j["fit_offsets_nm"][1])
        x = wl[a:b + 1]; y = R[a:b + 1]
        c = np.polyfit(x - x.mean(), y, 1)
        pred = c[0] * (jn + j["extrapolate_offset_nm"] - x.mean()) + c[1]
        other = R[K.idx(wl, jn + j["compare_offset_nm"])]
        den = R[K.idx(wl, jn + j["denominator_offset_nm"])]
        steps.append(abs(pred - other) / abs(den) if den != 0 else NAN)
    worst = float(np.nanmax(steps)) if steps else NAN
    o = "ok"
    if worst > b8["check_above"]:
        o = "Check"
    elif worst > b8["note_above"]:
        o = "Note"
    out["B8"] = {"status": "assessed", "relative_steps": [_f(s) for s in steps], "outcome": o}
    return out


UNAVAILABLE = "not available on this instrument"


def _window_ok(wl, lo, hi):
    """The N2 window lies on the scan's grid whole (with the neighbours its second difference reads)."""
    from spyder_ref import gains as GN
    return GN.window_covered(np.asarray(wl, float), lo, hi)


def _unavailable(why):
    return why if why.startswith(UNAVAILABLE) else f"{UNAVAILABLE} ({why})"


def stat_sd(st, n2c, wl, joins, ops, inoise, correlation_nm, win, what):
    """(SD of one detector term of a linear statistic on this scan, None) or (None, why not): the term's gain through
    the correction (spyder_ref.gains.stat_term_gain) x N2(win) on the AS-MEASURED scan (NaN when N2 is not finite)."""
    from spyder_ref import gains as GN
    try:
        g = GN.stat_term_gain(st, wl, joins, ops, inoise, correlation_nm, win, what)
    except GN.GainError as e:
        return None, e.status[len("not assessed: "):] if e.status.startswith("not assessed: ") else e.status
    return g * n2c(*win), None


def _stat(g, name):
    st = dict(g["statistics"][name])
    if isinstance(st["kernel"], str):
        st["kernel"] = g["kernels"][st["kernel"]]
    return st


BANDS_NOT_BUILT_IN = "not assessed: band evidence needs this instrument built in"


def band_evidence_off(ev, zo):
    """An instrument with no built-in correction (DECISIONS 137, amended 2026-10-19: uncalibrated instruments): the band
    calls are off. The evidence level and the ZooMS band pattern (pattern, verdict, the 1545 nm vote) become 'not
    assessed: band evidence needs this instrument built in' (null); the band readings themselves (E, u, SD, per-band
    state, lit / readable) stay for the close-ups and the export. In place."""
    ev["status"] = BANDS_NOT_BUILT_IN
    ev["level"] = None
    zo["status"] = BANDS_NOT_BUILT_IN
    for k in ("pattern", "verdict", "pattern_verdict", "vote_1545"):
        zo[k] = None


NATIVE_SCAN = "native standard-resolution scan"
NEAR_UNCORRECTED = "near-uncorrected correction (read as a native standard-resolution scan)"


def standard_like(Pa, Pl, Ps, wl, joins=(1000.0, 1800.0), ops=None, inoise=None):
    """Whether a correction leaves the stream's noise standard-resolution-like (DECISIONS 137; Matt, 2026-10-19:
    LabSpec 4 standard never changes): every statistic the noise policies read (check.acquisition's B6b gate and
    fallback statistics, check.longwave's tail reading, check.signs' soft-tier statistic) has a noise gain through the
    correction within std_like_within (check.acquisition) of its gain as measured, e.g. a standard-resolution unit's own
    file. Every noise policy then treats the stream exactly like a native standard-resolution scan: the uncorrected
    B6b table (no fallbacks), long-wave not assessed, the soft tier always eligible, the profiles' non-native models
    and second opinion off. A gain that cannot be computed: not near. No correction (ops None): False."""
    from spyder_ref import gains as GN
    if ops is None:
        return False
    g = Pa["B6b_signs_gate"]
    corr = g.get("correlation_nm", 1.25)
    terms = [(_stat(g, e["statistic"]), corr, tuple(e["n2_window_nm"]), f"check.acquisition {e['statistic']}")
             for e in g["corrected"].values() if e is not None]
    terms += [(_stat(g, f["statistic"]), corr, tuple(f["n2_window_nm"]), f"check.acquisition {f['statistic']}")
              for f in g.get("fallbacks", {}).values()]
    if Pl is not None and "tail_statistic" in Pl:
        terms.append((Pl["tail_statistic"], Pl.get("correlation_nm", 1.25), tuple(Pl["n2_window_nm"]),
                      "check.longwave tail_statistic"))
    st = (Ps or {}).get("soft_tier")
    if st:
        el = st["eligibility"]
        terms.append((el["otherwise_statistic"], el.get("correlation_nm", 1.25), tuple(el["otherwise_n2_window_nm"]),
                      "check.signs soft_tier"))
    try:
        return all(abs(GN.stat_term_gain(s, wl, joins, ops, inoise, c, w, what)
                       / GN.stat_term_gain(s, wl, joins, None, None, c, w, what) - 1.0) <= g["std_like_within"]
                   for s, c, w, what in terms)
    except GN.GainError:
        return False


def b6b(P, n2c, wl, joins=(1000.0, 1800.0), ops=None, inoise=None, near=False):
    """Per-sign B6b gates (phase 0c, DECISIONS 76; DECISIONS 137: for ANY correction), decided on the AS-MEASURED N2.

    The stream as measured (ops None), or through a near-uncorrected correction (near, standard_like): the uncorrected
    table, exactly today's standard-resolution gates (a sign is gated when N2 of its window exceeds n2_above; no
    fallbacks).
    Any other correction: a sign is gated when the propagated SD of its statistic's designated detector term exceeds
    sd_above (fixed once from the v0.2 cuts, in SD units); burnt (null) is never gated; a gated sign with a fallback
    runs its reduced form when the reduced statistic's SD is at most sd_at_most (DECISIONS 107). A window the scan does
    not cover: the sign is not checked, 'not available on this instrument (...)'.
    Returns (summary record, {sign: gate reason or None}, {sign: reduced variant}). Summary: n2 = N2(n2_window_nm),
    signs_gated = contaminant signs gated with no eligible fallback (not checked), signs_reduced = gated signs whose
    reduced variant is eligible (it runs, except C1 when a specific sign fired: C1 is then 'skipped' as usual),
    gated = every contaminant sign in signs_gated, outcome Note when either list is non-empty."""
    g = P["B6b_signs_gate"]
    corr = g.get("correlation_nm", 1.25)
    gates, reduced = {}, {}
    table = ops is None or bool(near)
    if table:
        for s in g["signs"]:
            e = g["uncorrected"].get(s)
            if e is None:
                gates[s] = None
                continue
            lo, hi = e["n2_window_nm"]
            if not _window_ok(wl, lo, hi):
                gates[s] = f"{UNAVAILABLE} (the N2 window {int(lo)}-{int(hi)} nm is not covered by the scan)"
                continue
            gates[s] = f"noise in {int(lo)}-{int(hi)} nm" if n2c(lo, hi) > e["n2_above"] else None
    else:
        for s in g["signs"]:
            e = g["corrected"].get(s)
            if e is None:                          # never gated (burnt: it reads the visible edge, not the SWIR2)
                gates[s] = None
                continue
            lo, hi = e["n2_window_nm"]
            sd, why = stat_sd(_stat(g, e["statistic"]), n2c, wl, joins, ops, inoise, corr, (lo, hi),
                              f"check.acquisition {e['statistic']}")
            if why is not None:
                gates[s] = _unavailable(why)
                continue
            gates[s] = f"noise in {int(lo)}-{int(hi)} nm" if sd > e["sd_above"] else None
        for s in g["contaminant_signs"]:
            f = g.get("fallbacks", {}).get(s)
            if not (f and gates.get(s)) or gates[s].startswith(UNAVAILABLE):
                continue
            sd, why = stat_sd(_stat(g, f["statistic"]), n2c, wl, joins, ops, inoise, corr, tuple(f["n2_window_nm"]),
                              f"check.acquisition {f['statistic']}")
            if why is None and sd <= f["sd_at_most"]:
                reduced[s] = f["variant"]
    v = n2c(*g["n2_window_nm"])
    sg = [s for s in g["contaminant_signs"] if gates.get(s) and s not in reduced]
    sr = [s for s in g["contaminant_signs"] if s in reduced]
    rec = {"status": "assessed", "n2": _f(v), "outcome": "Note" if (sg or sr) else "ok",
           "gated": len(sg) == len(g["contaminant_signs"]), "signs_gated": sg, "signs_reduced": sr}
    return rec, gates, reduced


def unusable(acq):
    return [k for k in ("B1", "B2", "B3", "B4") if acq.get(k, {}).get("outcome") == "Unusable"]


# ====================================================================== Step 2c: long-wave eligibility
def longwave(P, native, n2c, wl=None, joins=(1000.0, 1800.0), ops=None, inoise=None):
    """The long-wave tail (> 2300 nm) on any stream that is not the native standard-resolution scan (a correction
    applied, unless near-uncorrected; or another instrument's scan as measured; DECISIONS 137): tail-unreliable for B9 /
    C1 when the propagated SD of the tail reading (through the stream's correction; none as measured) x
    N2(n2_window_nm) exceeds b9_sd_above / c1_sd_above (fixed once from the v0.2 cuts, in SD units). native: why the
    stream counts as the native standard-resolution scan (NATIVE_SCAN or NEAR_UNCORRECTED; True means NATIVE_SCAN): not
    assessed, for that reason. A window or reading the scan does not cover: not available on this instrument, the tail
    unreliable."""
    lo, hi = P["n2_window_nm"]
    v = n2c(lo, hi)
    if native:
        why = native if isinstance(native, str) else NATIVE_SCAN
        return {"status": f"not assessed: {why}", "n2": _f(v), "tail_unreliable": False, "tail_unreliable_c1": False}
    sd, why = stat_sd(P["tail_statistic"], n2c, wl, joins, ops, inoise, P.get("correlation_nm", 1.25), (lo, hi),
                      "check.longwave tail_statistic")
    if why is not None:
        return {"status": f"not assessed: {_unavailable(why)}", "n2": _f(v), "tail_unreliable": True,
                "tail_unreliable_c1": True}
    return {"status": "assessed", "n2": _f(v), "tail_unreliable": bool(sd > P["b9_sd_above"]),
            "tail_unreliable_c1": bool(sd > P["c1_sd_above"])}


# ====================================================================== Step 5: B9
def b9(wl, R_std, P, prototypes, truncated=False):
    """truncated=True: the long-wave-free variant (windows and prototypes from P['truncated']), used when the
    long-wave tail is unreliable and the long-wave policy says 'truncated'."""
    Q = P["truncated"] if truncated else P
    E = K.derivative(R_std, P["kernel"])
    m = np.zeros(len(wl), bool)
    for lo, hi in Q["windows_nm"]:
        m |= (wl >= lo) & (wl <= hi)
    V = -E[:, m]                                   # sign as in the frozen prototype construction
    if not np.all(np.isfinite(V)):
        return {"status": "not assessed: kernel support", "score": None, "outcome": None}
    Z = (V - V.mean(1, keepdims=True)) / (V.std(1, keepdims=True) + P["std_epsilon"])
    s = float((Z @ prototypes.T / Z.shape[1]).max(1)[0])
    st = "assessed: truncated (long-wave region too noisy)" if truncated else "assessed"
    return {"status": st, "score": s, "outcome": "fail" if s < Q["fail_if_score_below"] else "pass"}


# ====================================================================== Step 7: organic-evidence level
def band_readings(wl, E, bands, ids):
    """r_b per band (a band with a projection is read on the projected E: the OH-corrected 1545 nm trough)."""
    return {b: float(K.band_value(E, wl, bands[b])[0]) for b in ids}


def _u(bands, b, r):
    """u_b = u_sign x r_b / w_b (u_sign -1 for a trough)."""
    return bands[b].get("u_sign", 1) * r / float(bands[b]["weight"])


def _nonfinite(*xs):
    return any(x is None or not math.isfinite(float(x)) for x in xs)


def evidence(wl, E, P, bands, sd_E):
    """Normative rule of 06_levels.py (frozen; A3). sd_E: {band: noise SD in E units} (gain x N2, keyed)."""
    core, extra = P["core_bands"], P["extra_band"]
    ids = core + [extra]
    r = band_readings(wl, E, bands, ids)
    g0 = P["guard"]
    if _nonfinite(*r.values(), K.window_max(E, wl, g0["window_nm"][0], g0["window_nm"][1])[0]):
        return {"status": "not assessed: non-finite band reading", "level": None, "S": None,
                "n_readable_core": None, "n_lit_core": None, "guard_max_E": None, "bands": {}}
    w = {b: float(bands[b]["weight"]) for b in ids}
    u = {b: _u(bands, b, r[b]) for b in ids}
    sdu = {b: _band_sd(sd_E, b) / w[b] for b in ids}
    sdmax = P["readability"]["sd_max_u"]; clear_t = P["thresholds_u"]["clear"]; strong_t = P["thresholds_u"]["strong"]
    unread_below = P["readability"]["unreadable_only_if_u_below"]

    def state(b, faint):
        if sdu[b] > sdmax and u[b] < unread_below:
            return "can't tell"
        if u[b] >= strong_t:
            return "strong"
        if u[b] >= clear_t:
            return "clear"
        if u[b] > faint:
            return "faint"
        return "flat"

    st = {b: state(b, bands[b]["faint_u"]) for b in core}
    st[extra] = state(extra, P["extra_band_faint_u"])
    readable = {b: st[b] != "can't tell" for b in core}
    ru = [u[b] for b in core if readable[b]]
    S = float(np.median(ru)) if ru else None
    nread = sum(readable.values())
    allflat = all(st[b] in ("flat", "can't tell") for b in core)
    # Step 7: a band is unreadable ("can't tell") only if SD_b > sd_max AND u_b < unreadable_only_if_u_below
    # (Codex Phase 3 review 4; the frozen research script used the SD condition alone for N-H 2044)
    nh_unreadable = st[extra] == "can't tell"
    nh_ok = (u[extra] <= P["none_rule"]["extra_band_max_u"]) or nh_unreadable
    g = P["guard"]
    guard_val = float(K.window_max(E, wl, g["window_nm"][0], g["window_nm"][1])[0])
    guard_ok = guard_val < g["max_E_below"]
    nr = P["none_rule"]
    none = (allflat and all(readable[b] for b in nr["must_be_readable"]) and nread >= nr["min_readable_core"]
            and nh_ok and guard_ok)
    lit = sum(st[b] in ("faint", "clear", "strong") for b in core)
    nh_vals = []
    for b in P["nh_type_bands"]:
        if b == extra:
            if not nh_unreadable:
                nh_vals.append(u[b])
        elif readable.get(b):
            nh_vals.append(u[b])
    nh_type = max(nh_vals) if nh_vals else None
    nh_ok_lev = nh_type is not None and nh_type >= P["nh_type_min_u"]
    strong = S is not None and S >= strong_t and lit == nread and nh_ok_lev
    clear = S is not None and S >= clear_t and lit >= P["clear_min_lit_core"] and nh_ok_lev
    nh_lit = (u[extra] > 0) and not nh_unreadable
    cant = (not none) and lit == 0 and allflat and guard_ok and not nh_lit
    level = ("none" if none else "can't tell" if cant else "strong" if strong else "clear" if clear else "trace")
    return {"status": "assessed", "level": level, "S": S, "n_readable_core": nread, "n_lit_core": lit,
            "guard_max_E": guard_val, "bands": {b: {"E": r[b], "u": u[b], "sd_u": _f(sdu[b]), "state": st[b]}
                                                 for b in ids}}


# ====================================================================== Step 9: ZooMS band pattern (A17)
def zooms(wl, E, P, bands, sd_E):
    prot, ch = P["protein_bands"], P["ch_bands"]
    six = prot + ch
    r = band_readings(wl, E, bands, six)
    if _nonfinite(*r.values()):
        return {"status": "not assessed: non-finite band reading", "pattern": None, "verdict": "Can't tell",
                "pattern_verdict": "Can't tell", "bands": {}, "vote_1545": False}
    rd, lit = {}, {}
    fr = P["readability"]
    for b in six:
        wb = float(bands[b]["weight"])
        sd = _band_sd(sd_E, b)
        rd[b] = not (sd > fr["sd_max_frac_w"] * wb and r[b] < fr["unreadable_only_if_E_below_frac_w"] * wb)
        lit[b] = r[b] > P["lit_if_E_above"]
    nP = sum(lit[b] for b in prot); nC = sum(lit[b] for b in ch)
    pat = "A" if nP + nC == 0 else "B" if nP == 0 else "C" if nP == 1 else "D" if nC < len(ch) else "E"
    p_rd = all(rd[b] for b in prot); p_flat = nP == 0
    good = all(rd.values()) and all(lit.values())
    if p_rd and p_flat:
        v = "Unlikely"
    elif good:
        v = "Good"
    elif p_rd and not p_flat and (nP == 1 or any((not lit[b]) and rd[b] for b in ch)):
        v = "Borderline"
    else:
        v = "Can't tell"
    # verdict = the ZooMS verdict (after the 1545 vote); pattern_verdict = the band pattern alone, which is what the
    # radiocarbon / isotopes rules read (+D, lift, positive signs): the vote belongs to ZooMS only (DECISIONS 75)
    out = {"status": "assessed", "pattern": pat, "verdict": v, "pattern_verdict": v,
           "bands": {b: {"E": r[b], "lit": bool(lit[b]), "readable": bool(rd[b]), "sd_E": _f(_band_sd(sd_E, b))}
                     for b in six}}
    # phase 0c vote (DECISIONS 75): a readable, lit OH-corrected 1545 nm trough turns Unlikely into Borderline
    vt = P.get("vote")
    vote = False
    if vt:
        b = vt["band"]
        rv = float(K.band_value(E, wl, bands[b])[0])
        uv = _u(bands, b, rv)
        sdu = _band_sd(sd_E, b) / float(bands[b]["weight"])
        fin = math.isfinite(rv)
        rdv = fin and not (sdu > fr["sd_max_frac_w"] and uv < fr["unreadable_only_if_E_below_frac_w"])
        litv = fin and uv > vt["lit_if_u_above"]
        vote = bool(v == vt["from_verdict"] and rdv and litv)
        if vote:
            out["verdict"] = vt["to_verdict"]
        out["vote_band"] = {"id": b, "E": _f(rv), "u": _f(uv), "sd_u": _f(sdu), "readable": bool(rdv), "lit": bool(litv)}
    out["vote_1545"] = vote
    return out


# ====================================================================== Step 8: contamination signs
def heat_range(P):
    """(lo, hi) nm the heat check reads: the visible edge from scan_from_nm up to the reference window, R_vis and the
    two OH sharp indices (450-1446 nm with the shipped parameters)."""
    e, c = P["charred"]["edge50"], P["calcined"]
    los = [e["scan_from_nm"], e["reference_nm"][0], c["R_vis"]["range_nm"][0]]
    his = [e["reference_nm"][1], c["R_vis"]["range_nm"][1]]
    for k in ("OH1433", "OH979"):
        cc, _, _, f1 = c[k]["sharp"]
        los.append(cc - f1)
        his.append(cc + f1)
    return float(min(los)), float(max(his))


def heat(wl, R_std, N17, P):
    """Charred by the visible edge alone; calcined. A scan whose grid does not hold the range the check reads (450-1446
    nm): 'not available on this instrument', never a default."""
    lo, hi = heat_range(P)
    wl = np.asarray(wl, float)
    on = lambda x: bool(np.any(np.abs(wl - x) <= 1e-6))
    if not (on(lo) and on(hi)):
        return {"status": f"not assessed: not available on this instrument (the heat sign reads {int(lo)}-{int(hi)} nm)",
                "edge50_nm": None, "OH1433": None, "OH979": None, "R_vis": None, "charred": None, "calcined": None,
                "fired": None}
    e = P["charred"]["edge50"]
    ref = K.window_mean(np.atleast_2d(R_std), wl, *e["reference_nm"])[0]
    a = K.idx(wl, e["scan_from_nm"])
    rel = np.asarray(R_std, float)[a:] / ref
    kk = np.flatnonzero(rel >= e["fraction"])
    edge50 = float(wl[a + kk[0]]) if len(kk) else None
    charred = edge50 is not None and edge50 >= P["charred"]["edge50_at_least_nm"]
    c = P["calcined"]
    oh1433 = float(K.sharp(N17, wl, *c["OH1433"]["sharp"])[0])
    oh979 = float(K.sharp(N17, wl, *c["OH979"]["sharp"])[0])
    rvis = float(K.window_mean(np.atleast_2d(R_std), wl, *c["R_vis"]["range_nm"])[0])
    if _nonfinite(ref, oh1433, oh979, rvis):
        # Codex Phase 3 review 3: a non-finite input is "not assessed", never "not burnt"
        return {"status": "not assessed: non-finite reading", "edge50_nm": edge50, "OH1433": _f(oh1433),
                "OH979": _f(oh979), "R_vis": _f(rvis), "charred": None, "calcined": None, "fired": None}
    calc = (not charred) and oh1433 >= c["OH1433"]["at_least"] and oh979 >= c["OH979"]["at_least"] \
        and rvis >= c["R_vis"]["at_least"]
    return {"edge50_nm": edge50, "OH1433": oh1433, "OH979": oh979, "R_vis": rvis, "charred": bool(charred),
            "calcined": bool(calc), "fired": bool(charred or calc)}


def gate_status(reason):
    """A sign not read because of its B6b gate: 'gated: <reason>'; because the scan does not cover its window: 'not
    assessed: not available on this instrument (...)'."""
    return f"not assessed: {reason}" if reason.startswith(UNAVAILABLE) else f"gated: {reason}"


def signs(wl, R_std, P, Ph, gates, reduced=None, heat_stream=None):
    """Plaster, wax, ester and burnt (no clay: DECISIONS 41). gates: {sign: B6b gate reason or None} (per sign,
    phase 0c); a gated sign is 'gated: <reason>' and not read, unless reduced names its fallback (plaster 'band_1446':
    the 1446 nm load alone, status 'assessed: reduced (1446 nm band only; <gate reason>)'). heat_stream: the spectrum
    the heat check reads when it is not R_std (the scan as measured, for a correction whose visible levels were not
    fitted; DECISIONS 137). Every sign record ends with gated (bool)."""
    names = ["plaster", "wax", "ester", "burnt"]
    gates = gates or {}
    reduced = reduced or {}
    out = {}
    if all(gates.get(n) for n in names) and not reduced.get("plaster"):
        return {n: {"status": gate_status(gates[n]), "fired": None, "gated": True} for n in names}
    N = K.derivative(R_std, P["kernel"])
    pl = P["plaster"]
    loads = [(float(K.sharp(N, wl, *b["sharp"])[0]) - b["fauna_median"]) / b["plaster_contrast"] for b in pl["bands"]]
    jl = min(loads)
    NA = "not assessed: non-finite reading"          # Codex Phase 3 review 3: never "not detected"
    if _nonfinite(*loads):
        out["plaster"] = {"status": NA, "joint_load": None, "fired": None}
    else:
        out["plaster"] = {"status": "assessed", "joint_load": jl, "fired": bool(jl > pl["fires_if_joint_load_above"])}
    stats, fired = {}, True
    for t in P["wax"]["all_of"]:
        v = float(K.sharp(N, wl, *t["sharp"])[0]); stats[str(t["sharp"][0])] = v
        fired &= v >= t["at_least"]
    if _nonfinite(*stats.values()):
        out["wax"] = {"status": NA, "sharp": {k: _f(x) for k, x in stats.items()}, "fired": None}
    else:
        out["wax"] = {"status": "assessed", "sharp": stats, "fired": bool(fired)}
    es = P["ester"]
    v = float(K.sharp(N, wl, *es["sharp"])[0])
    if _nonfinite(v):
        out["ester"] = {"status": NA, "sharp": None, "fired": None}
    else:
        out["ester"] = {"status": "assessed", "sharp": v, "fired": bool(v >= es["at_least"])}
    if heat_stream is None:
        hb = heat(wl, R_std, N, Ph)
    else:
        hb = heat(wl, heat_stream, K.derivative(heat_stream, P["kernel"]), Ph)
    out["burnt"] = {"status": "assessed", **hb}
    for n in names:
        if n == "plaster" and gates.get(n) and reduced.get(n) == "band_1446":
            r = pl["reduced_1446"]
            v = (float(K.sharp(N, wl, *r["sharp"])[0]) - r["fauna_median"]) / r["plaster_contrast"]
            if _nonfinite(v):
                out[n] = {"status": NA, "joint_load": None, "load_1446": None, "fired": None, "gated": False}
            else:
                out[n] = {"status": f"assessed: reduced (1446 nm band only; {gates[n]})", "joint_load": None,
                          "load_1446": v, "fired": bool(v > r["fires_if_load_above"]), "gated": False}
        elif gates.get(n):
            out[n] = {"status": gate_status(gates[n]), "fired": None, "gated": True}
        else:
            out[n]["gated"] = False
    return out


def soft_tier(Ps, Pc1, native, n2c, sg, c1r, wl=None, joins=(1000.0, 1800.0), ops=None, inoise=None):
    """The 'possible thin coating' tier (phase 0c, DECISIONS 74/76): a note only. Fires when no hard contaminant
    sign fired, the scan is eligible (the native standard-resolution scan always, also through a near-uncorrected
    correction; any other stream when the propagated SD of the C1 statistic's SWIR2 term, through its correction, x
    N2(otherwise_n2_window_nm) is at most
    otherwise_sd_at_most; DECISIONS 137) and a CHECKED sign's statistic reaches its soft line: C1 excess (full or
    truncated, check.c1), ester sharp, plaster joint load. components = the statistics that crossed (C1 / C1t, ester,
    plaster), only when it fires."""
    st = Ps.get("soft_tier")
    if not st:
        return {"status": "not assessed: no soft tier", "eligible": False, "fired": False, "components": []}
    hard = [n for n in st["requires_no_hard_sign"] if (c1r if n == "C1" else sg.get(n, {})).get("fired")]
    el = st["eligibility"]
    lo, hi = el["otherwise_n2_window_nm"]
    why = None
    if native and el.get("native_always", True):
        eligible = True
    else:
        sd, why = stat_sd(el["otherwise_statistic"], n2c, wl, joins, ops, inoise, el.get("correlation_nm", 1.25),
                          (lo, hi), "check.signs soft_tier")
        eligible = bool(why is None and sd <= el["otherwise_sd_at_most"])
    comps = []
    if eligible:
        c1s = str(c1r.get("status", ""))
        full, tr = Pc1.get("soft_tier"), (Pc1.get("truncated") or {}).get("soft_tier")
        if c1s == "assessed" and full and c1r["excess"] >= full["excess_at_least"]:
            comps.append("C1")
        elif c1s.startswith("assessed: truncated") and tr and c1r["excess"] >= tr["excess_at_least"]:
            comps.append("C1t")
        es, pl = sg.get("ester", {}), sg.get("plaster", {})
        if es.get("status") == "assessed" and es["sharp"] >= st["ester"]["at_least"]:
            comps.append("ester")
        if pl.get("status") == "assessed" and pl["joint_load"] >= st["plaster"]["joint_load_at_least"]:
            comps.append("plaster")
    fired = bool(eligible and not hard and comps)
    if eligible:
        status = "assessed"
    elif why is not None:
        status = f"not assessed: {_unavailable(why)}"
    else:
        status = f"not assessed: noise in {int(lo)}-{int(hi)} nm"
    return {"status": status, "eligible": eligible, "fired": fired, "components": comps if fired else []}


def c1(wl, E, P, gate, specific_fired, tail_unreliable, longwave_mode="not_assessed", reduced=None):
    """gate: C1's B6b gate reason (None = not gated). tail_unreliable: the long-wave tail is unreliable FOR C1
    (check.longwave tail_unreliable_c1). longwave_mode: what to do then: 'truncated' (the variant without the
    > 2300 nm band, P['truncated']) or 'not_assessed'. reduced: C1's fallback when gated ('swir1': P['swir1'], below
    1800 nm only, no film rule; status 'assessed: reduced (below 1800 nm only; <gate reason>)'). The record ends with
    gated (bool)."""
    nil = {"film": None, "film_fired": None}
    if gate and reduced != "swir1":
        return {"status": gate_status(gate), "excess": None, **nil, "fired": None, "gated": True}
    if specific_fired:
        return {"status": "skipped: a specific sign fired", "excess": None, **nil, "fired": None, "gated": False}
    Q, st = P, "assessed"
    if gate:
        Q, st = P["swir1"], f"assessed: reduced (below 1800 nm only; {gate})"
    elif tail_unreliable:
        if longwave_mode != "truncated":
            return {"status": "not assessed: long-wave region too noisy", "excess": None, **nil, "fired": None,
                    "gated": False}
        Q, st = P["truncated"], "assessed: truncated (long-wave region too noisy)"
    hw = P["read_half_width_nm"]
    rd = lambda c: float(K.band_reading(E, wl, c, hw)[0])
    ch = sum(rd(c) for c in Q["ch_bands_nm"]); nh = sum(rd(c) for c in Q["nh_bands_nm"])
    ex = ch - (Q["a"] + Q["b"] * nh)
    if not math.isfinite(ex):
        return {"status": "not assessed: kernel support", "excess": None, **nil, "fired": None, "gated": False}
    # the organic-film contrast (DECISIONS 103; full variant only): peak minus trough of the 2298/2325 feature
    film = film_fired = None
    fp = Q.get("film")
    if fp:
        fh = fp["half_width_nm"]
        f = float(K.band_reading(E, wl, fp["peak_nm"], fh)[0]) - float(K.band_reading(E, wl, fp["trough_nm"], fh)[0])
        if math.isfinite(f):
            film, film_fired = f, bool(f >= fp["fires_if_at_least"])
    fired = bool(ex > Q["fires_if_excess_above"]) or bool(film_fired)
    return {"status": st, "excess": ex, "film": film, "film_fired": film_fired, "fired": fired, "gated": False}
