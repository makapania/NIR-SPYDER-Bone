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
thin coating > contaminants not checked > models disagree > positive signs > bands too noisy). Every note that
applies is listed in notes_all (the details); c1_reduced (the truncated C1 ran) is details-only.
Phase 0c notes (DECISIONS 72, 76): above_bone_range when m > above_bone_range_m_above (every analysis);
possible_thin_coating when the soft tier fired (signs['soft']; it also suppresses the positive-signs line);
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
only when a slot is left (notes.precedence_last). No line when a sign in zooms_better.suppressed_by_signs FIRED (wax,
ester, plaster, C1: coatings can create the C-H bands the patterns read; the lift's blockers); burnt, gated or
unassessed signs do not suppress it. The line is shown after the rule's note and before the supporting note
(notes.precedence_zooms), still at most max_shown in all.
Ryder line (Matt, phase 0d REPORT_zooms_models): on an Unlikely verdict with no band line (Better / protein / 1545), when the
published Ryder 2045 model (the profile model zooms_better.ryder_2045_model_key, as read on this scan's stream) reads
>= zooms_better.ryder_2045_at_least (0.34, the paper's a priori ZooMS cut), zooms_better_ryder (params r, at_least) is the
ZooMS line; a faint protein note then stays in the details (the last tier shows only when no ZooMS line does). Suppressed by
the same fired signs.
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


def flags_for(prof, sg, c1r):
    contam, burnt = _signs_state(sg, c1r, prof["flags"])
    out = []
    if contam:
        out.append({"key": "flag_contaminant", "signs": contam})
    if burnt:
        out.append({"key": "flag_burnt", "signs": burnt})
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


def _common_notes(nt, m, sg, c1r):
    """(above_bone_range, possible_thin_coating) notes and the c1_reduced note, for every analysis."""
    first, last = [], []
    ab = nt.get("above_bone_range_m_above")
    if ab is not None and m is not None and m > ab:
        first.append({"key": "above_bone_range", "m": m})
    soft = sg.get("soft") or {}
    if soft.get("fired"):
        first.append({"key": "possible_thin_coating", "components": list(soft.get("components", []))})
    if str(c1r.get("status", "")).startswith("assessed: truncated"):
        last.append({"key": "c1_reduced"})
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


def zooms_better(nt, verdict, zo, step="model_verdict", fired=(), m=None, models=None):
    """The ZooMS notes (0, 1 or 2): the band line, or the Ryder 2045 line on an Unlikely verdict that has no band line
    (a faint protein note then follows it, for the details). models: the scan's models record (Ryder 2045's reading)."""
    zb = nt.get("zooms_better")
    if not zb or not zb.get("enabled"):
        return []
    if any(s in zb.get("suppressed_by_signs", []) for s in fired):
        return []
    band = _band_line(zb, verdict, zo, step, m)
    if band and band["key"] != "zooms_faint_protein":
        return [band]
    cut = zb.get("ryder_2045_at_least")
    if cut is not None and verdict == "Unlikely":
        r = ((models or {}).get(zb["ryder_2045_model_key"]) or {}).get("value")
        if r is not None and r >= cut:
            return [{"key": "zooms_better_ryder", "r": r, "at_least": cut}] + ([band] if band else [])
    return [band] if band else []


def collagen_verdict(prof, m, comps, ev, zo, sg, c1r, gated, models=None):
    """prof: the profile 'parameters' block. m: CONS3 (None if not available). comps: {name: value}. models: the
    scan's models record (the Ryder 2045 ZooMS line reads the published model there).
    ev: evidence result; zo: ZooMS pattern result; sg: signs (incl. 'soft'); c1r: C1 result; gated: every contaminant
    sign gated (the B6b summary; per-sign gating is read from the sign statuses)."""
    cuts = prof["cuts"]
    V = model_verdict(m, cuts)
    out = {"model_verdict": V, "notes_all": [], "flags": flags_for(prof, sg, c1r)}
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
    if fl["enabled"] and level == fl["evidence_level"]:
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
    notes += zooms_better(nt, v, zo, step, contam, m, models)
    first, last = _common_notes(nt, m, sg, c1r)
    soft_fired = any(n["key"] == "possible_thin_coating" for n in first)
    notes += first
    if promising and unchecked:
        notes.append({"key": "contaminants_not_checked", "skipped": skipped, "checked": checked})
    if models_disagree(comps, cuts, nt):
        notes.append({"key": "models_disagree", "components": dict(comps)})
    if promising and not contam and not unchecked and not soft_fired:
        ps = nt["positive_signs"]
        if zv == ps["all_six_zooms_verdict"]:
            notes.append({"key": "positive_signs_all_six"})
        elif level in ps["clear_levels"] and m >= ps["clear_min_m"]:
            notes.append({"key": "positive_signs_clear"})
    if level == nt["bands_too_noisy_level"]:
        notes.append({"key": "bands_too_noisy"})
    notes += last
    out["notes_all"] = notes
    out.update(verdict=v, rule_step=step)
    out["notes_shown"] = shown_notes(notes, nt, lifted=step.startswith("lift_") and step != "lift_blocked")
    return out


def _base_key(k):
    return {"lift_good": "lift", "lift_borderline": "lift", "lift_blocked_contaminant": "lift_blocked",
            "lift_blocked_noisy": "lift_blocked", "positive_signs_all_six": "positive_signs",
            "positive_signs_clear": "positive_signs", "zooms_better_good": "zooms_better",
            "zooms_better_protein": "zooms_better", "zooms_better_1545": "zooms_better",
            "zooms_better_1545_flat": "zooms_better", "zooms_better_ryder": "zooms_better"}.get(k, k)


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
    for p in nt["precedence_supporting"]:
        if lifted and p == "positive_signs" and nt.get("positive_signs_hidden_after_lift", True):
            continue
        hit = [k for k in keys if _base_key(k) == p]
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
    return shown[: nt["max_shown"]]


def zooms_verdict(prof, zo, sg, c1r, m=None):
    """The band-pattern verdict (incl. its 1545 nm vote). With a 'notes' block: above_bone_range and
    possible_thin_coating (one shown, profile order), c1_reduced details-only."""
    out = {"verdict": zo["verdict"], "rule_step": "zooms_pattern", "pattern": zo["pattern"],
           "flags": flags_for(prof, sg, c1r), "notes_all": [], "notes_shown": []}
    nt = prof.get("notes")
    if nt:
        first, last = _common_notes(nt, m, sg, c1r)
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
