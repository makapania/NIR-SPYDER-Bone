//! Step 9: verdicts per analysis type from an analysis profile (port of `reference/oracle/verdict.py`;
//! PLAN Step 9; DECISIONS 15, 16, 45, 52, 53).
//!
//! Radiocarbon and isotopes (rule L2): the model verdict from m (CONS3) and the cuts; flat bands lead (evidence
//! "none" -> Unlikely); +D (ZooMS pattern verdict Unlikely and m >= 3 -> Borderline); the lift (strong -> Good,
//! clear -> at least Borderline) only with ZooMS Good, no plaster/wax/ester/C1 sign and every one of them checked
//! (none gated by its own B6b gate; C1 assessed), otherwise the would-be lift is "blocked"; flags (contaminant,
//! burnt) are badges and NEVER change the verdict.
//! Notes: the rule's note first (+D > flat bands lead > lift > blocked lift), then one supporting note (profile
//! order: above bone range > possible thin coating > contaminants not checked > models disagree > positive signs >
//! bands too noisy); all in `notes_all`; c1_reduced is details-only. Phase 0c (DECISIONS 72, 76): above_bone_range
//! (m above the profile's line, every analysis), possible_thin_coating (the soft tier, signs.soft; it suppresses
//! the positive-signs line), contaminants_not_checked with params skipped / checked.
//! Models disagree (DECISIONS 65): the three CONS3 components fall in different verdict bands (straddle a cut)
//! and span at least `models_disagree_min_span` points.
//! ZooMS line (DECISIONS 80 amended): the ZooMS band-pattern rule (its 1545 nm vote included) keeps running
//! underneath; where it calls the scan BETTER than this verdict (Good > Borderline > Unlikely; Can't tell never
//! counts), a note says so and never changes the verdict: zooms_better_good (ZooMS Good), zooms_better_protein
//! (ZooMS Borderline from the band pattern), zooms_better_1545 (ZooMS Borderline only through the 1545 vote;
//! zooms_better_1545_flat when flat bands set the verdict). No line when a sign of `suppressed_by_signs` FIRED (wax,
//! ester, plaster, C1: coatings can create the C-H bands); burnt, gated or unassessed signs do not suppress it. The
//! band-pattern Borderline splits by strength (Matt): zooms_better_protein only with m >= the profile's line AND at
//! least two N-type bands (NH2044, AM2175, the 1545 vote band) lit and readable; else the quieter
//! zooms_faint_protein (params: the lit N-type bands, m), shown only when a slot is left (`precedence_last`). It is
//! shown after the rule's note and before the supporting note (`precedence_zooms`), at most `max_shown` in all.
//! Ryder line (Matt, phase 0d REPORT_zooms_models): on an Unlikely verdict with no band line, when the published Ryder
//! 2045 model reads >= the profile's cut (0.34, the paper's a priori ZooMS cut), zooms_better_ryder (params r, at_least) is
//! the ZooMS line; a faint protein note then stays in the details. Suppressed by the same fired signs.

use super::val::{strs, Obj, V};
use crate::plugins::profiles::{AnalysisProfile, ModelsDisagreeWhen, ProfileParams, RuleL2};

/// What the verdict rules read from the signs, C1, evidence and ZooMS results.
#[derive(Debug, Clone)]
pub struct VerdictInputs<'a> {
    /// The signs record (plaster, wax, ester, burnt -> {status, fired, ...}; soft -> {fired, components}).
    pub signs: &'a Obj,
    /// The C1 record ({status, fired, ...}).
    pub c1: &'a Obj,
    pub level: &'a str,
    /// The band-pattern verdict (before the 1545 vote): what the radiocarbon / isotopes rules read.
    pub zooms_verdict: &'a str,
    /// The ZooMS verdict as the ZooMS profile shows it (after the 1545 vote; DECISIONS 75).
    pub zooms_shown_verdict: &'a str,
    pub zooms_pattern: &'a str,
    /// The ZooMS check record (`bands` lit / readable, `vote_band`): the protein line's strength test.
    pub zooms_rec: &'a Obj,
    /// The scan's models record (the Ryder 2045 ZooMS line reads the published model there).
    pub models: &'a Obj,
    /// Every contaminant sign gated (the B6b summary); per-sign gating is read from the sign statuses.
    pub gated: bool,
}

fn rank(v: &str) -> i32 {
    match v {
        "Unlikely" => 0,
        "Borderline" => 1,
        "Good" => 2,
        _ => -1,
    }
}

fn name(r: i32) -> &'static str {
    match r {
        0 => "Unlikely",
        1 => "Borderline",
        _ => "Good",
    }
}

pub fn model_verdict(m: Option<f64>, p: &RuleL2) -> Option<&'static str> {
    let m = m?;
    Some(if m < p.unlikely_below {
        "Unlikely"
    } else if m < p.good_from {
        "Borderline"
    } else {
        "Good"
    })
}

/// The models-disagree note (DECISIONS 65): every CONS3 component read, and they fall in different verdict
/// bands of the profile's own cuts (< unlikely_below, unlikely_below <= x < good_from, >= good_from), AND their span
/// (max - min) is at least `models_disagree_min_span` (DECISIONS 65 amended: smaller spreads are model error).
pub fn models_disagree(comps: &[(String, Option<f64>)], p: &RuleL2) -> bool {
    match p.notes.models_disagree_when {
        ModelsDisagreeWhen::ComponentsInDifferentVerdictBands => {
            let vals: Vec<f64> = comps.iter().filter_map(|(_, x)| *x).collect();
            if vals.is_empty() || vals.len() != comps.len() {
                return false;
            }
            let first = model_verdict(Some(vals[0]), p);
            let bands_differ = vals.iter().any(|&x| model_verdict(Some(x), p) != first);
            let mx = vals.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let mn = vals.iter().copied().fold(f64::INFINITY, f64::min);
            bands_differ && mx - mn >= p.notes.models_disagree_min_span
        }
    }
}

fn fired(rec: &Obj) -> bool {
    rec.get("fired").is_some_and(V::truthy)
}

/// (contaminant signs fired, burnt signs fired), in the profile's flag order.
fn signs_state(prof: &AnalysisProfile, x: &VerdictInputs) -> (Vec<String>, Vec<String>) {
    let get = |n: &str| -> bool {
        if n == "C1" {
            fired(x.c1)
        } else {
            x.signs.get(n).and_then(V::as_obj).is_some_and(fired)
        }
    };
    let contam = prof
        .flags
        .contaminant
        .iter()
        .filter(|n| get(n))
        .cloned()
        .collect();
    let burnt = prof
        .flags
        .burnt
        .iter()
        .filter(|n| x.signs.get(n).and_then(V::as_obj).is_some_and(fired))
        .cloned()
        .collect();
    (contam, burnt)
}

/// The flag badges ({key, signs}) beside the verdict.
pub fn flags_for(prof: &AnalysisProfile, x: &VerdictInputs) -> V {
    let (contam, burnt) = signs_state(prof, x);
    let mut out = Vec::new();
    if !contam.is_empty() {
        out.push(V::Map(
            Obj::new()
                .with("key", "flag_contaminant")
                .with("signs", strs(&contam)),
        ));
    }
    if !burnt.is_empty() {
        out.push(V::Map(
            Obj::new()
                .with("key", "flag_burnt")
                .with("signs", strs(&burnt)),
        ));
    }
    V::List(out)
}

fn status_of<'a>(x: &'a VerdictInputs, n: &str) -> &'a str {
    let o = if n == "C1" {
        Some(x.c1)
    } else {
        x.signs.get(n).and_then(V::as_obj)
    };
    o.and_then(|o| o.get("status"))
        .and_then(V::as_str)
        .unwrap_or("")
}

/// (skipped = gated contaminant signs, checked = assessed ones; C1 also when skipped because a specific sign
/// fired), in the profile's flag order.
fn contaminant_check_state(
    prof: &AnalysisProfile,
    x: &VerdictInputs,
) -> (Vec<String>, Vec<String>) {
    let names = &prof.flags.contaminant;
    let skipped = names
        .iter()
        .filter(|n| status_of(x, n).starts_with("gated"))
        .cloned()
        .collect();
    let checked = names
        .iter()
        .filter(|n| {
            let st = status_of(x, n);
            st.starts_with("assessed") || (n.as_str() == "C1" && st.starts_with("skipped"))
        })
        .cloned()
        .collect();
    (skipped, checked)
}

/// (above_bone_range, possible_thin_coating) and (c1_reduced): the notes every analysis shares.
fn common_notes(above: Option<f64>, m: Option<f64>, x: &VerdictInputs) -> (Vec<Obj>, Vec<Obj>) {
    let mut first = Vec::new();
    let mut last = Vec::new();
    if let (Some(ab), Some(m)) = (above, m) {
        if m > ab {
            first.push(Obj::new().with("key", "above_bone_range").with("m", m));
        }
    }
    if let Some(soft) = x.signs.get("soft").and_then(V::as_obj) {
        if fired(soft) {
            let comps = soft.get("components").cloned().unwrap_or(V::List(vec![]));
            first.push(
                Obj::new()
                    .with("key", "possible_thin_coating")
                    .with("components", comps),
            );
        }
    }
    if status_of(x, "C1").starts_with("assessed: truncated") {
        last.push(Obj::new().with("key", "c1_reduced"));
    }
    (first, last)
}

fn keys_of(notes: &[Obj]) -> Vec<String> {
    notes
        .iter()
        .map(|n| n.get("key").and_then(V::as_str).unwrap_or("").to_string())
        .collect()
}

fn base_key(k: &str) -> &str {
    match k {
        "lift_good" | "lift_borderline" => "lift",
        "lift_blocked_contaminant" | "lift_blocked_noisy" => "lift_blocked",
        "positive_signs_all_six" | "positive_signs_clear" => "positive_signs",
        "zooms_better_good"
        | "zooms_better_protein"
        | "zooms_better_1545"
        | "zooms_better_1545_flat"
        | "zooms_better_ryder" => "zooms_better",
        other => other,
    }
}

fn shown_notes(keys: &[String], p: &RuleL2, lifted: bool) -> Vec<String> {
    let nt = &p.notes;
    let mut shown = Vec::new();
    for pr in &nt.precedence_rule {
        if let Some(k) = keys.iter().find(|k| base_key(k) == pr) {
            shown.push(k.clone());
            break;
        }
    }
    // the ZooMS line ranks after the rule's note and before the supporting note (DECISIONS 80 amended)
    for pr in &nt.precedence_zooms {
        if let Some(k) = keys.iter().find(|k| base_key(k) == pr) {
            shown.push(k.clone());
            break;
        }
    }
    for pr in &nt.precedence_supporting {
        if lifted && pr == "positive_signs" && nt.positive_signs_hidden_after_lift {
            continue;
        }
        if let Some(k) = keys.iter().find(|k| base_key(k) == pr) {
            shown.push(k.clone());
            break;
        }
    }
    // the quietest notes (the faint protein sign): only when a slot is left and no ZooMS line shows
    let zooms_shown = shown
        .iter()
        .any(|k| nt.precedence_zooms.iter().any(|z| z == base_key(k)));
    for pr in &nt.precedence_last {
        if let Some(k) = keys.iter().find(|k| base_key(k) == pr) {
            if shown.len() < nt.max_shown && !zooms_shown {
                shown.push(k.clone());
            }
            break;
        }
    }
    shown.truncate(nt.max_shown);
    shown
}

/// A ZooMS-check band lit AND readable: one of the six (`bands`) or the 1545 vote band (`vote_band`).
fn band_lit(z: &Obj, b: &str) -> bool {
    let ok =
        |o: &Obj| o.get("lit").is_some_and(V::truthy) && o.get("readable").is_some_and(V::truthy);
    if let Some(o) = z
        .get("bands")
        .and_then(V::as_obj)
        .and_then(|bs| bs.get(b))
        .and_then(V::as_obj)
    {
        return ok(o);
    }
    match z.get("vote_band").and_then(V::as_obj) {
        Some(v) if v.get("id").and_then(V::as_str) == Some(b) => ok(v),
        _ => false,
    }
}

/// The ZooMS line's note key (DECISIONS 80 amended) when the ZooMS verdict (after the 1545 nm vote) ranks above
/// `verdict`; None otherwise (ZooMS Can't tell, Rescan or not bone never count), or when a suppressing contaminant
/// sign fired (`fired`: the contaminant signs that fired). `step`: the rule step that set the verdict; `m`: CONS3.
pub fn zooms_better(
    p: &RuleL2,
    verdict: &str,
    step: &str,
    fired: &[String],
    m: f64,
    x: &VerdictInputs,
) -> Vec<Obj> {
    if !p.notes.zooms_better
        || fired
            .iter()
            .any(|s| p.notes.zooms_better_suppressed_by.contains(s))
    {
        return Vec::new();
    }
    let band = band_line(p, verdict, step, m, x);
    if let Some(b) = &band {
        if b.get("key").and_then(V::as_str) != Some("zooms_faint_protein") {
            return vec![b.clone()];
        }
    }
    if let Some((key, cut)) = &p.notes.zooms_ryder {
        let r = x
            .models
            .get(key)
            .and_then(V::as_obj)
            .and_then(|o| o.get("value"))
            .and_then(V::as_f64)
            .filter(|r| r.is_finite());
        if let (Some(r), "Unlikely") = (r, verdict) {
            if r >= *cut {
                let mut out = vec![Obj::new()
                    .with("key", "zooms_better_ryder")
                    .with("r", r)
                    .with("at_least", *cut)];
                out.extend(band);
                return out;
            }
        }
    }
    band.into_iter().collect()
}

/// The band line (Better / protein / 1545 / faint) when the ZooMS check ranks above `verdict`.
fn band_line(p: &RuleL2, verdict: &str, step: &str, m: f64, x: &VerdictInputs) -> Option<Obj> {
    let zs = x.zooms_shown_verdict;
    let (rz, rv) = (rank(zs), rank(verdict));
    if rz < 0 || rv < 0 || rz <= rv {
        return None;
    }
    let key = if zs == "Good" {
        "zooms_better_good"
    } else if x.zooms_verdict != zs {
        // the 1545 vote turned an Unlikely pattern into Borderline
        if step == "flat_bands_lead" {
            "zooms_better_1545_flat"
        } else {
            "zooms_better_1545"
        }
    } else {
        // no strength test in this profile: every band-pattern Borderline is the protein line
        let Some(pt) = &p.notes.zooms_protein else {
            return Some(Obj::new().with("key", "zooms_better_protein"));
        };
        let lit: Vec<String> = pt
            .n_type_bands
            .iter()
            .filter(|b| band_lit(x.zooms_rec, b))
            .cloned()
            .collect();
        if m >= pt.m_at_least && lit.len() >= pt.n_type_lit_at_least {
            "zooms_better_protein"
        } else {
            return Some(
                Obj::new()
                    .with("key", "zooms_faint_protein")
                    .with("lit", strs(&lit))
                    .with("m", m),
            );
        }
    };
    Some(Obj::new().with("key", key))
}

/// Rule L2 for radiocarbon / isotopes. `comps`: the CONS3 component readings (name, value) in profile order.
pub fn collagen_verdict(
    prof: &AnalysisProfile,
    p: &RuleL2,
    m: Option<f64>,
    comps: &[(String, Option<f64>)],
    x: &VerdictInputs,
) -> Obj {
    let mv = model_verdict(m, p);
    let mut out = Obj::new()
        .with("model_verdict", mv)
        .with("notes_all", V::List(vec![]))
        .with("flags", flags_for(prof, x));
    let (Some(v0), Some(m)) = (mv, m) else {
        out.set("verdict", "Can't tell");
        out.set("rule_step", "no_model_reading");
        out.set("notes_shown", V::List(vec![]));
        out.set("primary_note", V::Null);
        out.set(
            "notes_all",
            V::List(vec![V::Map(Obj::new().with("key", "no_model_reading"))]),
        );
        return out;
    };
    let (contam, _burnt) = signs_state(prof, x);
    let c1_status = x.c1.get("status").and_then(V::as_str).unwrap_or("");
    let c1_ok = c1_status.starts_with("assessed") || c1_status.starts_with("skipped");
    // a contaminant sign that could not be assessed (non-finite reading) leaves the contaminants unchecked
    let not_assessed = prof
        .flags
        .contaminant
        .iter()
        .filter(|n| *n != "C1")
        .any(|n| {
            x.signs
                .get(n)
                .and_then(V::as_obj)
                .and_then(|o| o.get("status"))
                .and_then(V::as_str)
                .is_some_and(|s| s.starts_with("not assessed"))
        });
    let (skipped, checked) = contaminant_check_state(prof, x);
    let unchecked = x.gated || !skipped.is_empty() || !c1_ok || not_assessed;
    let mut v = v0.to_string();
    let mut step = "model_verdict".to_string();
    let mut notes: Vec<Obj> = Vec::new();
    let (fl, pd, lf) = (&p.flat_bands_lead, &p.plus_d, &p.lift);
    if fl.enabled && x.level == fl.evidence_level {
        v = fl.verdict.clone();
        step = "flat_bands_lead".into();
        notes.push(Obj::new().with("key", "flat_bands_lead").with("m", m));
    } else if pd.enabled && x.zooms_verdict == pd.zooms_verdict && m >= pd.m_at_least {
        v = pd.verdict.clone();
        step = "plus_d".into();
        notes.push(Obj::new().with("key", "plus_d").with("m", m));
    } else {
        let mut tgt = v0;
        if x.zooms_verdict == lf.requires_zooms_verdict {
            if lf.to_good_levels.iter().any(|l| l == x.level) {
                tgt = "Good";
            } else if lf
                .to_at_least_borderline_levels
                .iter()
                .any(|l| l == x.level)
            {
                tgt = name(rank(v0).max(rank("Borderline")));
            }
        }
        if rank(tgt) > rank(v0) {
            let blockers: Vec<String> = contam
                .iter()
                .filter(|s| lf.blocked_by_signs.contains(s))
                .cloned()
                .collect();
            if blockers.is_empty() && !(unchecked && lf.requires_contaminants_checked) {
                v = tgt.to_string();
                step = if tgt == "Good" {
                    "lift_good".into()
                } else {
                    "lift_borderline".into()
                };
                notes.push(
                    Obj::new()
                        .with("key", step.as_str())
                        .with("m", m)
                        .with("level", x.level),
                );
            } else if !blockers.is_empty() {
                step = "lift_blocked".into();
                notes.push(
                    Obj::new()
                        .with("key", "lift_blocked_contaminant")
                        .with("m", m)
                        .with("level", x.level)
                        .with("signs", strs(&blockers)),
                );
            } else {
                step = "lift_blocked".into();
                notes.push(
                    Obj::new()
                        .with("key", "lift_blocked_noisy")
                        .with("m", m)
                        .with("level", x.level),
                );
            }
        }
    }
    let promising = v == "Good" || v == "Borderline";
    let nt = &p.notes;
    notes.extend(zooms_better(p, &v, &step, &contam, m, x));
    let (first, last) = common_notes(nt.above_bone_range_m_above, Some(m), x);
    let soft_fired = keys_of(&first).iter().any(|k| k == "possible_thin_coating");
    notes.extend(first);
    if promising && unchecked {
        notes.push(
            Obj::new()
                .with("key", "contaminants_not_checked")
                .with("skipped", strs(&skipped))
                .with("checked", strs(&checked)),
        );
    }
    if models_disagree(comps, p) {
        let mut c = Obj::new();
        for (k, x) in comps {
            c.set(k, *x);
        }
        notes.push(
            Obj::new()
                .with("key", "models_disagree")
                .with("components", c),
        );
    }
    if promising && contam.is_empty() && !unchecked && !soft_fired {
        if x.zooms_verdict == nt.positive_signs_all_six_zooms_verdict {
            notes.push(Obj::new().with("key", "positive_signs_all_six"));
        } else if nt.positive_signs_clear_levels.iter().any(|l| l == x.level)
            && m >= nt.positive_signs_clear_min_m
        {
            notes.push(Obj::new().with("key", "positive_signs_clear"));
        }
    }
    if nt.bands_too_noisy_level.as_deref() == Some(x.level) {
        notes.push(Obj::new().with("key", "bands_too_noisy"));
    }
    notes.extend(last);
    let keys = keys_of(&notes);
    let lifted = step.starts_with("lift_") && step != "lift_blocked";
    out.set(
        "notes_all",
        V::List(notes.into_iter().map(V::Map).collect()),
    );
    out.set("verdict", v);
    out.set("rule_step", step);
    out.set("notes_shown", strs(&shown_notes(&keys, p, lifted)));
    out
}

/// The ZooMS verdict: the band-pattern verdict of the check (DECISIONS 17), including its 1545 nm vote. With a
/// notes block: above_bone_range and possible_thin_coating (one shown, profile order), c1_reduced details-only.
pub fn zooms_verdict(prof: &AnalysisProfile, m: Option<f64>, x: &VerdictInputs) -> Obj {
    let mut out = Obj::new()
        .with("verdict", x.zooms_shown_verdict)
        .with("rule_step", "zooms_pattern")
        .with("pattern", x.zooms_pattern)
        .with("flags", flags_for(prof, x))
        .with("notes_all", V::List(vec![]))
        .with("notes_shown", V::List(vec![]));
    if let ProfileParams::ZoomsPattern {
        notes: Some(nt), ..
    } = &prof.params
    {
        let (first, last) = common_notes(nt.above_bone_range_m_above, m, x);
        let notes: Vec<Obj> = first.into_iter().chain(last).collect();
        let keys = keys_of(&notes);
        let mut shown = Vec::new();
        for pr in &nt.precedence_supporting {
            if let Some(k) = keys.iter().find(|k| base_key(k) == pr) {
                shown.push(k.clone());
                break;
            }
        }
        shown.truncate(nt.max_shown);
        out.set(
            "notes_all",
            V::List(notes.into_iter().map(V::Map).collect()),
        );
        out.set("notes_shown", strs(&shown));
    }
    out
}

/// The verdict for one profile (rule L2 or ZooMS pattern).
pub fn verdict_for(
    prof: &AnalysisProfile,
    m: Option<f64>,
    comps: &[(String, Option<f64>)],
    x: &VerdictInputs,
) -> Obj {
    match &prof.params {
        ProfileParams::CollagenRuleL2(p) => collagen_verdict(prof, p, m, comps, x),
        ProfileParams::ZoomsPattern { .. } => zooms_verdict(prof, m, x),
    }
}

/// "Most promising first": (verdict group ascending, value descending within it; unrounded).
pub fn sort_key(
    prof: &AnalysisProfile,
    verdict: &str,
    m: Option<f64>,
    s: Option<f64>,
) -> (usize, Option<f64>) {
    let order = &prof.sort.verdict_order;
    let g = order
        .iter()
        .position(|o| o == verdict)
        .unwrap_or(order.len());
    let val = match prof.sort.within.get(verdict).map(String::as_str) {
        Some("m") => m,
        Some("S") => s,
        _ => None,
    };
    (g, val)
}
