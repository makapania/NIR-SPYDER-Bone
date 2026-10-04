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


def acquisition(wl, R, P, dn=None):
    """B2-B5, B7, B8 on the as-measured reflectance (B1 is the reader's matrix; B6/B6b are in pipeline)."""
    wl = np.asarray(wl, float); R = np.asarray(R, float)
    out = {}
    lo, hi = P["integrity_range_nm"]
    k = (wl >= lo) & (wl <= hi)
    # B2 saturation (needs raw DN)
    b2 = P["B2_saturation"]
    if dn is None:
        out["B2"] = {"status": "not assessed: no raw DN (spectrum input)", "outcome": None}
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
    k7 = (wl >= b7["range_nm"][0]) & (wl <= b7["range_nm"][1])
    mx = float(np.max(R[k7]))
    out["B7"] = {"status": "assessed", "max_R": _f(mx), "outcome": "Note" if mx > b7["max_R_above"] else "ok"}
    # B8 splice steps at the header joins (both line fits on the SWIR1 side)
    b8 = P["B8_splice_step"]
    steps = []
    for j in b8["fits"]:
        a, b = K.idx(wl, j["fit_nm"][0]), K.idx(wl, j["fit_nm"][1])
        x = wl[a:b + 1]; y = R[a:b + 1]
        c = np.polyfit(x - x.mean(), y, 1)
        pred = c[0] * (j["extrapolate_to_nm"] - x.mean()) + c[1]
        other = R[K.idx(wl, j["compare_nm"])]
        den = R[K.idx(wl, j["denominator_nm"])]
        steps.append(abs(pred - other) / abs(den) if den != 0 else NAN)
    worst = float(np.nanmax(steps)) if steps else NAN
    o = "ok"
    if worst > b8["check_above"]:
        o = "Check"
    elif worst > b8["note_above"]:
        o = "Note"
    out["B8"] = {"status": "assessed", "relative_steps": [_f(s) for s in steps], "outcome": o}
    return out


def b6b(P, n2c, tkey="none"):
    """Per-sign B6b gates (phase 0c, DECISIONS 76), decided on the AS-MEASURED N2 once the stream's transfer key is
    known: the table for `tkey` ('none' or the transfer file's sha256; a key without an entry uses
    unknown_transfer_key_uses). Returns (summary record, {sign: gate reason or None}). Summary: n2 = N2(n2_window_nm),
    gated = every contaminant sign gated, signs_gated = the gated contaminant signs, outcome Note when any is gated."""
    g = P["B6b_signs_gate"]
    tab = g["by_transfer_key"].get(tkey)
    if tab is None:
        tab = g["by_transfer_key"][g["unknown_transfer_key_uses"]]
    gates = {}
    for s in g["signs"]:
        e = tab.get(s)
        if e is None:                          # a null gate: the sign is never gated (burnt on v0.2)
            gates[s] = None
            continue
        lo, hi = e["n2_window_nm"]
        gates[s] = f"noise in {int(lo)}-{int(hi)} nm" if n2c(lo, hi) > e["n2_above"] else None
    v = n2c(*g["n2_window_nm"])
    sg = [s for s in g["contaminant_signs"] if gates.get(s)]
    rec = {"status": "assessed", "n2": _f(v), "outcome": "Note" if sg else "ok",
           "gated": len(sg) == len(g["contaminant_signs"]), "signs_gated": sg}
    return rec, gates


def unusable(acq):
    return [k for k in ("B1", "B2", "B3", "B4") if acq.get(k, {}).get("outcome") == "Unusable"]


# ====================================================================== Step 2c: long-wave eligibility
def longwave(P, instrument_class, n2c):
    lo, hi = P["n2_window_nm"]
    v = n2c(lo, hi)
    applies = instrument_class in P["applies_to_classes"]
    unrel = bool(applies and v > P["tail_unreliable_if_n2_above"])
    # per-check cut (phase 0c): C1 (the contaminant check only) has its own, higher cut; B9 keeps the shared one
    unrel_c1 = bool(applies and v > P.get("c1_tail_unreliable_if_n2_above", P["tail_unreliable_if_n2_above"]))
    return {"status": "assessed" if applies else "not assessed: standard-resolution class",
            "n2": _f(v), "tail_unreliable": unrel, "tail_unreliable_c1": unrel_c1}


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
    sdu = {b: (sd_E[b] / w[b] if sd_E.get(b) is not None else NAN) for b in ids}
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
        sd = sd_E.get(b)
        sd = NAN if sd is None else sd
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
           "bands": {b: {"E": r[b], "lit": bool(lit[b]), "readable": bool(rd[b]), "sd_E": _f(sd_E.get(b, NAN))}
                     for b in six}}
    # phase 0c vote (DECISIONS 75): a readable, lit OH-corrected 1545 nm trough turns Unlikely into Borderline
    vt = P.get("vote")
    vote = False
    if vt:
        b = vt["band"]
        rv = float(K.band_value(E, wl, bands[b])[0])
        uv = _u(bands, b, rv)
        sd = sd_E.get(b)
        sdu = NAN if sd is None else sd / float(bands[b]["weight"])
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
def heat(wl, R_std, N17, P):
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


def signs(wl, R_std, P, Ph, gates):
    """Plaster, wax, ester and burnt (no clay: DECISIONS 41). gates: {sign: B6b gate reason or None} (per sign,
    phase 0c); a gated sign is 'gated: <reason>' and not read. Every sign record ends with gated (bool)."""
    names = ["plaster", "wax", "ester", "burnt"]
    gates = gates or {}
    out = {}
    if all(gates.get(n) for n in names):
        return {n: {"status": f"gated: {gates[n]}", "fired": None, "gated": True} for n in names}
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
    hb = heat(wl, R_std, N, Ph)
    out["burnt"] = {"status": "assessed", **hb}
    for n in names:
        if gates.get(n):
            out[n] = {"status": f"gated: {gates[n]}", "fired": None, "gated": True}
        else:
            out[n]["gated"] = False
    return out


def soft_tier(Ps, Pc1, instrument_class, n2c, sg, c1r):
    """The 'possible thin coating' tier (phase 0c, DECISIONS 74/76): a note only. Fires when no hard contaminant
    sign fired, the scan is eligible (a class in always_classes, else N2(otherwise_n2_window_nm) <= otherwise_n2_at_most)
    and a CHECKED sign's statistic reaches its soft line: C1 excess (full or truncated, check.c1), ester sharp, plaster
    joint load. components = the statistics that crossed (C1 / C1t, ester, plaster), only when it fires."""
    st = Ps.get("soft_tier")
    if not st:
        return {"status": "not assessed: no soft tier", "eligible": False, "fired": False, "components": []}
    hard = [n for n in st["requires_no_hard_sign"] if (c1r if n == "C1" else sg.get(n, {})).get("fired")]
    el = st["eligibility"]
    lo, hi = el["otherwise_n2_window_nm"]
    eligible = bool(instrument_class in el["always_classes"] or n2c(lo, hi) <= el["otherwise_n2_at_most"])
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
    return {"status": "assessed" if eligible else f"not assessed: noise in {int(lo)}-{int(hi)} nm",
            "eligible": eligible, "fired": fired, "components": comps if fired else []}


def c1(wl, E, P, gate, specific_fired, tail_unreliable, longwave_mode="not_assessed"):
    """gate: C1's B6b gate reason (None = not gated). tail_unreliable: the long-wave tail is unreliable FOR C1
    (check.longwave tail_unreliable_c1). longwave_mode: what to do then: 'truncated' (the variant without the
    > 2300 nm band, P['truncated']) or 'not_assessed'. The record ends with gated (bool)."""
    if gate:
        return {"status": f"gated: {gate}", "excess": None, "fired": None, "gated": True}
    if specific_fired:
        return {"status": "skipped: a specific sign fired", "excess": None, "fired": None, "gated": False}
    Q, st = P, "assessed"
    if tail_unreliable:
        if longwave_mode != "truncated":
            return {"status": "not assessed: long-wave region too noisy", "excess": None, "fired": None, "gated": False}
        Q, st = P["truncated"], "assessed: truncated (long-wave region too noisy)"
    hw = P["read_half_width_nm"]
    rd = lambda c: float(K.band_reading(E, wl, c, hw)[0])
    ch = sum(rd(c) for c in Q["ch_bands_nm"]); nh = sum(rd(c) for c in Q["nh_bands_nm"])
    ex = ch - (Q["a"] + Q["b"] * nh)
    if not math.isfinite(ex):
        return {"status": "not assessed: kernel support", "excess": None, "fired": None, "gated": False}
    return {"status": st, "excess": ex, "fired": bool(ex > Q["fires_if_excess_above"]), "gated": False}
