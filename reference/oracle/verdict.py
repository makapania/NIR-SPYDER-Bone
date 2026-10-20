"""Step 9: verdicts per analysis type, from an analysis-profile file (PLAN v3.2 Step 9; DECISIONS 15, 16, 45, 52, 53).

Radiocarbon and isotopes (rule L2):
  1. model verdict V from m (= CONS3) and the profile cuts;
  2. flat bands lead: evidence level 'none' -> Unlikely whatever m reads;
  3. +D: ZooMS pattern verdict Unlikely (both protein bands readable and flat) and m >= 3 -> Borderline;
  4. lift, only on a scan with no contaminant sign (plaster, wax, ester, C1; burnt does not block), every one of them
     CHECKED (none gated by its own B6b gate, none 'not assessed'; C1 assessed or skipped because a specific sign
     fired), with the ZooMS pattern verdict Good: strong -> Good, clear -> at least Borderline;
     otherwise the would-be lift is 'blocked' (contaminant, or too noisy to check);
  5. flags (contaminant, burnt) are badges beside the verdict and NEVER change it (DECISIONS 53; no rule C);
  6. otherwise V.
Notes: at most `max_shown` under the verdict: first the note of the rule that set the verdict (profile order:
+D > flat bands lead > lift > blocked lift), then ONE supporting note (profile order: above bone range > possible
thin coating > C-H note > readers disagree strongly > contaminants not checked > models disagree > positive signs >
bands too noisy). The strong warnings (notes.warnings: ch_above_nh_strong, readers_disagree_strongly) never take the
supporting slot: when one fires it is shown after the other notes, outside max_shown (DECISIONS 134). Every note that
applies is listed in notes_all (the details); c1_reduced (the truncated C1 ran) and signs_reduced (plaster or C1 ran a
reduced fallback below 1800 nm on a noisy scan; params: signs) are details-only.
Phase 0c notes (DECISIONS 72, 76): above_bone_range when m > above_bone_range_m_above (every analysis);
possible_thin_coating when the soft tier fired (signs['soft']; it also suppresses the positive-signs line);
a panel note listed in positive_signs.suppressed_by_notes (the C-H note, readers disagree strongly) suppresses the
positive-signs line too: no encouraging line against conflicting evidence (DECISIONS 128);
contaminants_not_checked carries skipped (gated signs) and checked (assessed signs).
Models disagree (DECISIONS 65): the three CONS3 components fall in different verdict bands (straddle a cut)
and span at least models_disagree_min_span points.
ZooMS line (DECISIONS 80 amended): the ZooMS band-pattern rule (check.zooms_patterns, its 1545 nm vote included) keeps
running underneath; where it calls the scan BETTER than this verdict (Good > Borderline > Unlikely; Can't tell never
counts), a note says so and never changes the verdict: zooms_better_good (ZooMS Good), zooms_better_protein (ZooMS
Borderline from the band pattern), zooms_better_1545 (ZooMS Borderline only through the 1545 vote; zooms_better_1545_flat
when flat bands set the verdict). The band-pattern Borderline splits by strength (Matt): zooms_better_protein only
with m >= zooms_better.protein.m_at_least AND at least n_type_lit_at_least of the N-type bands (NH2044, AM2175, the
1545 vote band) lit and readable; otherwise the quieter zooms_faint_protein (params: the lit N-type bands, m), shown
only when a slot is left (notes.precedence_last). No line when a note in zooms_better.suppressed_by_notes fired (the C-H
note, DECISIONS 120), nor when a sign in zooms_better.suppressed_by_signs FIRED (wax,
ester, plaster, C1: coatings can create the C-H bands the patterns read; the lift's blockers); burnt, gated or
unassessed signs do not suppress it. The line is shown after the rule's note and before the supporting note
(notes.precedence_zooms), still at most max_shown in all.
Ryder line (Matt, phase 0d REPORT_zooms_models): on an Unlikely verdict with no band line (Better / protein / 1545), when the
published Ryder 2045 model (the profile model zooms_better.ryder_2045_model_key, as read on this scan's stream) reads
>= zooms_better.ryder_2045_at_least (0.34, the paper's a priori ZooMS cut), zooms_better_ryder (params r, at_least) is the
ZooMS line; a faint protein note then stays in the details (the last tier shows only when no ZooMS line does). Suppressed by
the same fired signs.
Panel notes (DECISIONS 111, 115, 119; notes.panel; see panel_notes): ch_above_nh_possible / ch_above_nh_strong (the C-H 1728 nm
reader above CONS3, two levels) and readers_disagree_strongly (any of six reader contrasts far beyond clean bone; when it
fires, models_disagree is not added). Notes only: they never change the verdict, the lift or the sort.
Uncalibrated instruments (DECISIONS 137, amended 2026-10-19: no correction for the scan's instrument is installed,
uncalibrated=True): the band calls are off. The model verdict stands (no flat bands lead, no +D, no lift, no blocked
lift); no ZooMS band line and no faint protein note (the Ryder line, a model reading, stays); no positive-signs line
and no bands-too-noisy note (both read the bands); the flags still show, each marked provisional (they still set the
sort penalty), and so does the soft contaminant note (possible_thin_coating, marked provisional); the ZooMS profile says Can't tell (rule step band_evidence_not_built_in, no pattern).
The oracle emits note KEYS and their parameters; the UI holds the strings.
"""
from __future__ import annotations

RANK = {"Unlikely": 0, "Borderline": 1, "Good": 2}
NAMES = {0: "Unlikely", 1: "Borderline", 2: "Good"}


def model_verdict(m, cuts):
    if m is None:
        return None
    if m < cuts["unlikely_below"]:
        return "Unlikely"
    if m < cuts["good_from"]:
        return "Borderline"
    return "Good"


def models_disagree(comps, cuts, nt):
    """The models-disagree note (DECISIONS 65): all CONS3 components read and they fall in different verdict bands
    of the profile's own cuts (< unlikely_below, unlikely_below <= x < good_from, >= good_from), AND their span
    (max - min) is at least models_disagree_min_span (DECISIONS 65 amended: smaller spreads are within model error)."""
    rule = nt["models_disagree_when"]
    if rule != "components_in_different_verdict_bands":
        raise ValueError(f"models_disagree_when: unknown rule {rule!r}")
    vals = [x for x in comps.values() if x is not None]
    if not vals or len(vals) != len(comps):
        return False
    return len({model_verdict(x, cuts) for x in vals}) > 1 and max(vals) - min(vals) >= nt["models_disagree_min_span"]


def _signs_state(sg, c1r, flag_sets):
    contam = [n for n in flag_sets["contaminant"] if (c1r if n == "C1" else sg.get(n, {})).get("fired")]
    burnt = [n for n in flag_sets["burnt"] if sg.get(n, {}).get("fired")]
    return contam, burnt


def flags_for(prof, sg, c1r, provisional=False):
    """provisional: the instrument has no built-in correction, so its contaminant and heat signs are provisional."""
    contam, burnt = _signs_state(sg, c1r, prof["flags"])
    out = []
    if contam:
        out.append({"key": "flag_contaminant", "signs": contam})
    if burnt:
        out.append({"key": "flag_burnt", "signs": burnt})
    if provisional:
        for f in out:
            f["provisional"] = True
    return out


def _status(sg, c1r, n):
    return str((c1r if n == "C1" else sg.get(n, {})).get("status", ""))


def _contaminant_check_state(prof, sg, c1r):
    """(skipped = gated contaminant signs, checked = assessed ones (C1 also when skipped because a specific sign
    fired)), in the profile's flag order."""
    names = prof["flags"]["contaminant"]
    skipped = [n for n in names if _status(sg, c1r, n).startswith("gated")]
    checked = [n for n in names if _status(sg, c1r, n).startswith("assessed")
               or (n == "C1" and _status(sg, c1r, n).startswith("skipped"))]
    return skipped, checked


def _common_notes(nt, m, sg, c1r, provisional=False):
    """(above_bone_range, possible_thin_coating) notes and the c1_reduced / signs_reduced notes, for every analysis.
    provisional: the instrument has no built-in correction, so the soft contaminant note (possible_thin_coating) is
    marked provisional, as the flags are."""
    first, last = [], []
    ab = nt.get("above_bone_range_m_above")
    if ab is not None and m is not None and m > ab:
        first.append({"key": "above_bone_range", "m": m})
    soft = sg.get("soft") or {}
    if soft.get("fired"):
        first.append({"key": "possible_thin_coating", "components": list(soft.get("components", []))})
        if provisional:
            first[-1]["provisional"] = True
    if str(c1r.get("status", "")).startswith("assessed: truncated"):
        last.append({"key": "c1_reduced"})
    red = [n for n in ("plaster", "C1") if _status(sg, c1r, n).startswith("assessed: reduced")]
    if red:
        last.append({"key": "signs_reduced", "signs": red})
    return first, last


def band_lit(zo, b):
    """A ZooMS-check band lit AND readable: one of the six (zo['bands']) or the 1545 vote band (zo['vote_band'])."""
    d = (zo.get("bands") or {}).get(b)
    if d is None and (zo.get("vote_band") or {}).get("id") == b:
        d = zo["vote_band"]
    return bool(d) and bool(d.get("lit")) and bool(d.get("readable"))


def _band_line(zb, verdict, zo, step, m):
    """The ZooMS line (DECISIONS 80 amended): its note when the ZooMS band-pattern verdict (after the 1545 vote) ranks
    above this verdict, else None. zo: the ZooMS check record ('verdict' after the vote, 'pattern_verdict' the pattern
    alone, 'bands', 'vote_band'); step: the rule step that set the verdict; fired: the contaminant signs that FIRED (a
    suppressing one means no line); m: CONS3 (the protein line's strength test)."""
    zs = zo["verdict"]
    if zs not in RANK or verdict not in RANK or RANK[zs] <= RANK[verdict]:
        return None
    if zs == "Good":
        return {"key": "zooms_better_good"}
    if zo.get("pattern_verdict", zs) != zs:          # the 1545 vote turned an Unlikely pattern into Borderline
        return {"key": "zooms_better_1545_flat" if step == "flat_bands_lead" else "zooms_better_1545"}
    pr = zb.get("protein")
    if pr is None:                                   # no strength test in this profile: every pattern Borderline
        return {"key": "zooms_better_protein"}
    if not pr.get("n_type_bands"):                   # the Rust loader rejects it too (an empty list would pass all)
        raise ValueError("zooms_better.protein.n_type_bands: at least one band")
    lit = [b for b in pr["n_type_bands"] if band_lit(zo, b)]
    if m is not None and m >= pr["m_at_least"] and len(lit) >= pr["n_type_lit_at_least"]:
        return {"key": "zooms_better_protein"}
    return {"key": "zooms_faint_protein", "lit": lit, "m": m}


def zooms_better(nt, verdict, zo, step="model_verdict", fired=(), m=None, models=None, notes_fired=(),
                 bands=True):
    """The ZooMS notes (0, 1 or 2): the band line, or the Ryder 2045 line on an Unlikely verdict that has no band line
    (a faint protein note then follows it, for the details). models: the scan's models record (Ryder 2045's reading).
    notes_fired: base keys of the panel notes that fired; one in zooms_better.suppressed_by_notes (the C-H note,
    DECISIONS 120) suppresses the line as a fired sign does. bands=False (an uncalibrated instrument): no band line and
    no faint protein note; only the Ryder line (a model reading) can show."""
    zb = nt.get("zooms_better")
    if not zb or not zb.get("enabled"):
        return []
    if any(s in zb.get("suppressed_by_signs", []) for s in fired):
        return []
    if any(n in zb.get("suppressed_by_notes", []) for n in notes_fired):
        return []
    band = _band_line(zb, verdict, zo, step, m) if bands else None
    if band and band["key"] != "zooms_faint_protein":
        return [band]
    cut = zb.get("ryder_2045_at_least")
    if cut is not None and verdict == "Unlikely":
        r = ((models or {}).get(zb["ryder_2045_model_key"]) or {}).get("value")
        if r is not None and r >= cut:
            return [{"key": "zooms_better_ryder", "r": r, "at_least": cut}] + ([band] if band else [])
    return [band] if band else []


PANEL_CONTRASTS = ("CH", "CHN", "GL", "SHP", "SP3", "RAW")


def _panel_bin(edges, m):
    return sum(1 for e in edges if m >= e)


def panel_notes(nt, m, comps, models):
    """The panel notes (DECISIONS 111, 115, 119; notes.panel; never change the verdict). Returns (notes, readers-disagree
    fired). ch_above_nh: the C-H reader (readers.ch) minus m above possible_above / strong_above of m's bin ->
    ch_above_nh_possible / ch_above_nh_strong (params ch, m). readers_disagree_strongly: the six contrasts CH = ch - m,
    CHN = m - ch, GL = m - narrow1545, SHP = wc1500 - narrow1545, SP3 = max - min of the CONS3 parts, RAW = |m - raw2045|,
    each (x - median) / (q98 - median) of m's bin; fires when any present one exceeds level and the contrast itself exceeds
    min_gap points (0 when absent; a gap under a point is noise near zero, DECISIONS 133) (CH skipped when the C-H note
    fired; a contrast with a missing reading is skipped); params contrasts (in that order), m.
    Also returns the verdict block's `panel` record (DECISIONS 121): ch, ch_gap, ch_possible_above / ch_strong_above for m's
    bin, and contrasts [{name, value, fires_above, fraction}] (1 = at the line), CH included."""
    pn = nt.get("panel")
    if not pn or m is None:
        return [], False, None
    rd = pn["readers"]
    val = lambda k: ((models or {}).get(rd[k]) or {}).get("value")
    ch, nb, raw = val("ch"), val("narrow1545"), val("raw2045")
    out, ch_fired = [], False
    c = pn.get("ch_above_nh")
    rec = {"ch": ch, "ch_gap": None if ch is None else ch - m}
    if c:
        bc = _panel_bin(c["cons3_bin_edges"], m)
        rec["ch_possible_above"] = c["possible_above"][bc]
        rec["ch_strong_above"] = c["strong_above"][bc]
    if c and ch is not None:
        b = _panel_bin(c["cons3_bin_edges"], m)
        g = ch - m
        if g > c["strong_above"][b]:
            out.append({"key": "ch_above_nh_strong", "ch": ch, "m": m}); ch_fired = True
        elif g > c["possible_above"][b]:
            out.append({"key": "ch_above_nh_possible", "ch": ch, "m": m}); ch_fired = True
    r = pn.get("readers_disagree_strongly")
    if not r:
        return out, False, rec
    parts = [comps.get(k) for k in ("wc2045", "wc1500", "F05")]
    w1500 = comps.get("wc1500")
    gaps = {"CH": None if ch is None else ch - m, "CHN": None if ch is None else m - ch,
            "GL": None if nb is None else m - nb, "SHP": None if nb is None or w1500 is None else w1500 - nb,
            "SP3": None if any(x is None for x in parts) else max(parts) - min(parts),
            "RAW": None if raw is None else abs(m - raw)}
    b = _panel_bin(r["cons3_bin_edges"], m)
    listed = []
    for name in PANEL_CONTRASTS:
        sc = r["contrasts"].get(name)
        if sc is None or gaps[name] is None:
            continue
        med, q = sc["median"][b], sc["q98"][b]
        line = max(med + r["level"] * (q - med), r.get("min_gap", 0.0))
        listed.append({"name": name, "value": gaps[name], "fires_above": line,
                       "fraction": (gaps[name] - med) / (line - med)})
    rec["contrasts"] = listed
    fired = []
    for name in PANEL_CONTRASTS:
        sc = r["contrasts"].get(name)
        if sc is None or gaps[name] is None or (name == "CH" and ch_fired):
            continue
        med, q = sc["median"][b], sc["q98"][b]
        # the clean-bone line of m's bin, and at least min_gap points: a gap under a point is noise near zero (DECISIONS 133)
        if (gaps[name] - med) / (q - med) > r["level"] and gaps[name] > r.get("min_gap", 0.0):
            fired.append(name)
    if fired:
        out.append({"key": "readers_disagree_strongly", "contrasts": fired, "m": m})
    return out, bool(fired), rec


def collagen_verdict(prof, m, comps, ev, zo, sg, c1r, gated, models=None, uncalibrated=False):
    """prof: the profile 'parameters' block. m: CONS3 (None if not available). comps: {name: value}. models: the
    scan's models record (the Ryder 2045 ZooMS line reads the published model there).
    ev: evidence result; zo: ZooMS pattern result; sg: signs (incl. 'soft'); c1r: C1 result; gated: every contaminant
    sign gated (the B6b summary; per-sign gating is read from the sign statuses). uncalibrated: no correction for the
    scan's instrument is installed: the band calls are off (module notes)."""
    cuts = prof["cuts"]
    V = model_verdict(m, cuts)
    out = {"model_verdict": V, "notes_all": [], "flags": flags_for(prof, sg, c1r, provisional=uncalibrated)}
    if V is None:
        out.update(verdict="Can't tell", rule_step="no_model_reading", notes_shown=[], primary_note=None)
        out["notes_all"].append({"key": "no_model_reading"})
        return out
    # the band-pattern verdict BEFORE the ZooMS-only 1545 vote (DECISIONS 75: the vote is a ZooMS vote)
    level = ev["level"]; zv = zo.get("pattern_verdict", zo["verdict"])
    contam, burnt = _signs_state(sg, c1r, prof["flags"])
    c1_ok = str(c1r.get("status", "")).startswith(("assessed", "skipped"))
    # a contaminant sign that could not be assessed (non-finite reading) leaves the contaminants unchecked
    not_assessed = any(str(sg.get(n, {}).get("status", "")).startswith("not assessed")
                       for n in prof["flags"]["contaminant"] if n != "C1")
    skipped, checked = _contaminant_check_state(prof, sg, c1r)
    unchecked = bool(gated) or bool(skipped) or not c1_ok or not_assessed
    v, step, primary = V, "model_verdict", None
    notes = []
    fl = prof["flat_bands_lead"]; pdp = prof["plus_d"]; lf = prof["lift"]
    if uncalibrated:
        pass                                     # the band calls are off: the model verdict stands
    elif fl["enabled"] and level == fl["evidence_level"]:
        v, step = fl["verdict"], "flat_bands_lead"
        notes.append({"key": "flat_bands_lead", "m": m})
    elif pdp["enabled"] and zv == pdp["zooms_verdict"] and m >= pdp["m_at_least"]:
        v, step = pdp["verdict"], "plus_d"
        notes.append({"key": "plus_d", "m": m})
    else:
        tgt = V
        if zv == lf["requires_zooms_verdict"]:
            if level in lf["to_good_levels"]:
                tgt = "Good"
            elif level in lf["to_at_least_borderline_levels"]:
                tgt = NAMES[max(RANK[V], RANK["Borderline"])]
        if RANK[tgt] > RANK[V]:
            blockers = [s for s in contam if s in lf["blocked_by_signs"]]
            if not blockers and not (unchecked and lf["requires_contaminants_checked"]):
                v = tgt
                step = "lift_good" if tgt == "Good" else "lift_borderline"
                notes.append({"key": step, "m": m, "level": level})
            elif blockers:
                step = "lift_blocked"
                notes.append({"key": "lift_blocked_contaminant", "m": m, "level": level, "signs": blockers})
            else:
                step = "lift_blocked"
                notes.append({"key": "lift_blocked_noisy", "m": m, "level": level})
    promising = v in ("Good", "Borderline")
    nt = prof["notes"]
    # the panel notes first: a fired C-H note can suppress the ZooMS line (DECISIONS 120); listed after the common notes
    pnotes, strong_disagree, panel_rec = panel_notes(nt, m, comps, models)
    notes += zooms_better(nt, v, zo, step, contam, m, models, [_base_key(n["key"]) for n in pnotes],
                          bands=not uncalibrated)
    first, last = _common_notes(nt, m, sg, c1r, provisional=uncalibrated)
    soft_fired = any(n["key"] == "possible_thin_coating" for n in first)
    notes += first
    notes += pnotes
    if promising and unchecked:
        notes.append({"key": "contaminants_not_checked", "skipped": skipped, "checked": checked})
    if not strong_disagree and models_disagree(comps, cuts, nt):
        notes.append({"key": "models_disagree", "components": dict(comps)})
    ps = nt["positive_signs"]
    # no encouraging line when a panel note says the evidence conflicts (DECISIONS 128)
    panel_suppresses = any(_base_key(n["key"]) in ps.get("suppressed_by_notes", []) for n in pnotes)
    if promising and not contam and not unchecked and not soft_fired and not panel_suppresses and not uncalibrated:
        if zv == ps["all_six_zooms_verdict"]:
            notes.append({"key": "positive_signs_all_six"})
        elif level in ps["clear_levels"] and m >= ps["clear_min_m"]:
            notes.append({"key": "positive_signs_clear"})
    if level == nt["bands_too_noisy_level"] and not uncalibrated:
        notes.append({"key": "bands_too_noisy"})
    notes += last
    out["notes_all"] = notes
    out.update(verdict=v, rule_step=step)
    out["notes_shown"] = shown_notes(notes, nt, lifted=step.startswith("lift_") and step != "lift_blocked")
    if panel_rec is not None:
        out["panel"] = panel_rec
    return out


def _base_key(k):
    return {"lift_good": "lift", "lift_borderline": "lift", "lift_blocked_contaminant": "lift_blocked",
            "lift_blocked_noisy": "lift_blocked", "positive_signs_all_six": "positive_signs",
            "positive_signs_clear": "positive_signs", "zooms_better_good": "zooms_better",
            "zooms_better_protein": "zooms_better", "zooms_better_1545": "zooms_better",
            "zooms_better_1545_flat": "zooms_better", "zooms_better_ryder": "zooms_better",
            "ch_above_nh_possible": "ch_above_nh", "ch_above_nh_strong": "ch_above_nh"}.get(k, k)


def shown_notes(notes, nt, lifted=False):
    keys = [n["key"] for n in notes]
    shown = []
    for p in nt.get("precedence_rule", []):
        hit = [k for k in keys if _base_key(k) == p]
        if hit:
            shown.append(hit[0])
            break
    # the ZooMS line ranks after the rule's note and before the supporting note (DECISIONS 80 amended)
    for p in nt.get("precedence_zooms", []):
        hit = [k for k in keys if _base_key(k) == p]
        if hit:
            shown.append(hit[0])
            break
    warnings = nt.get("warnings", [])
    for p in nt["precedence_supporting"]:
        if lifted and p == "positive_signs" and nt.get("positive_signs_hidden_after_lift", True):
            continue
        hit = [k for k in keys if _base_key(k) == p and k not in warnings]
        if hit:
            shown.append(hit[0])
            break
    # the quietest notes (the faint protein sign): only when a slot is left and no ZooMS line shows
    zooms_shown = any(_base_key(k) in nt.get("precedence_zooms", []) for k in shown)
    for p in nt.get("precedence_last", []):
        hit = [k for k in keys if _base_key(k) == p]
        if hit and len(shown) < nt["max_shown"] and not zooms_shown:
            shown.append(hit[0])
            break
    # the strong warnings: always shown when they fire, outside the slots and max_shown (DECISIONS 134)
    return shown[: nt["max_shown"]] + [w for w in warnings if w in keys]


def zooms_verdict(prof, zo, sg, c1r, m=None, uncalibrated=False):
    """The band-pattern verdict (incl. its 1545 nm vote). With a 'notes' block: above_bone_range and
    possible_thin_coating (one shown, profile order), c1_reduced and signs_reduced details-only. uncalibrated: no
    band pattern on this instrument: Can't tell, rule step band_evidence_not_built_in, no pattern."""
    if uncalibrated:
        out = {"verdict": "Can't tell", "rule_step": "band_evidence_not_built_in", "pattern": None,
               "flags": flags_for(prof, sg, c1r, provisional=True), "notes_all": [], "notes_shown": []}
    else:
        out = {"verdict": zo["verdict"], "rule_step": "zooms_pattern", "pattern": zo["pattern"],
               "flags": flags_for(prof, sg, c1r), "notes_all": [], "notes_shown": []}
    nt = prof.get("notes")
    if nt:
        first, last = _common_notes(nt, m, sg, c1r, provisional=uncalibrated)
        out["notes_all"] = first + last
        out["notes_shown"] = shown_notes(out["notes_all"], nt)
    return out


def sort_key(prof, verdict, m, S):
    """(group, value): ascending group, descending value within it ('most promising first'). Unrounded values."""
    s = prof["sort"]
    order = s["verdict_order"]
    g = order.index(verdict) if verdict in order else len(order)
    by = s["within"].get(verdict)
    val = {"m": m, "S": S}.get(by) if by else None
    return g, val


def sort_penalty(prof, out):
    """DECISIONS 99: 1 when a flag of the verdict block names one of the profile's sort.penalty_signs (the scan sorts
    after the unpenalised scans of its own verdict group), else 0; None when the profile has no penalty_signs."""
    signs = prof["sort"].get("penalty_signs")
    if signs is None:
        return None
    bad = [s for s in signs if s not in prof["flags"]["contaminant"]]
    if bad:
        raise ValueError(f"sort.penalty_signs: {bad} not among the profile's contaminant flag signs")
    fired = {s for f in out.get("flags", []) for s in f.get("signs", [])}
    return int(bool(fired & set(signs)))
