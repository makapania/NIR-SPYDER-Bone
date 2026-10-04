//! Engine checks (port of `reference/oracle/checks.py`; PLAN Steps 2, 2c, 5, 7, 8, 9). Each takes one scan and
//! its parameters (from the check files) and returns the oracle's record dict. Every check reports a status
//! separately from its result: "assessed" | "not assessed: <reason>" | "gated: <reason>".

use std::collections::BTreeMap;

use super::kernels::{band_reading, band_value, derivative, idx, sharp, window_max, window_mean};
use super::val::{fin, Obj, V};
use crate::n2::n2;
use crate::numsum::{np_mean, np_std};
use crate::plugins::checks::{B8Fit, B9Variant, C1Variant, CheckParams, Sharp};
use crate::plugins::tables::{Bands, Kernel};

/// N2 per window on the AS-MEASURED scan, computed once per window; `table()` lists every window used.
pub struct N2Cache<'a> {
    wl: &'a [f64],
    r: &'a [f64],
    v: Vec<((f64, f64), f64)>,
}

impl<'a> N2Cache<'a> {
    pub fn new(wl: &'a [f64], r: &'a [f64]) -> Self {
        N2Cache {
            wl,
            r,
            v: Vec::new(),
        }
    }

    pub fn get(&mut self, lo: f64, hi: f64) -> f64 {
        if let Some((_, x)) = self.v.iter().find(|(k, _)| *k == (lo, hi)) {
            return *x;
        }
        let x = n2(self.r, self.wl, lo, hi).unwrap_or(f64::NAN);
        self.v.push(((lo, hi), x));
        x
    }

    /// {"<lo>-<hi>": N2} sorted by window (int() of the bounds, as the oracle prints them).
    pub fn table(&self) -> Obj {
        let mut v = self.v.clone();
        v.sort_by(|a, b| a.0 .0.total_cmp(&b.0 .0).then(a.0 .1.total_cmp(&b.0 .1)));
        let mut o = Obj::new();
        for ((lo, hi), x) in v {
            o.set(
                &format!("{}-{}", lo.trunc() as i64, hi.trunc() as i64),
                fin(x),
            );
        }
        o
    }
}

/// Status of a check whose reading is not finite: "not assessed", never "not detected".
const NOT_FINITE: &str = "not assessed: non-finite reading";

fn in_range(w: f64, lo: f64, hi: f64) -> bool {
    w >= lo && w <= hi
}

/// numpy `np.nanmax` (NaN if all NaN or empty).
fn nanmax(v: impl Iterator<Item = f64>) -> f64 {
    let mut m = f64::NAN;
    for x in v {
        if !x.is_nan() && (m.is_nan() || x > m) {
            m = x;
        }
    }
    m
}

/// numpy `np.max` (NaN propagates).
fn npmax(v: impl Iterator<Item = f64>) -> f64 {
    let mut m = f64::NEG_INFINITY;
    let mut any = false;
    for x in v {
        any = true;
        if x.is_nan() {
            return f64::NAN;
        }
        if x > m {
            m = x;
        }
    }
    if any {
        m
    } else {
        f64::NAN
    }
}

/// np.polyfit(x - x.mean(), y, 1) evaluated at `at - x.mean()` (closed-form least squares).
fn line_eval(x: &[f64], y: &[f64], at: f64) -> f64 {
    let xm = np_mean(x);
    let xc: Vec<f64> = x.iter().map(|v| v - xm).collect();
    let xcm = np_mean(&xc);
    let ym = np_mean(y);
    let mut sxy = 0.0;
    let mut sxx = 0.0;
    for (a, b) in xc.iter().zip(y) {
        sxy += (a - xcm) * (b - ym);
        sxx += (a - xcm) * (a - xcm);
    }
    let slope = sxy / sxx;
    let intercept = ym - slope * xcm;
    slope * (at - xm) + intercept
}

/// B2-B5, B7, B8 on the as-measured reflectance (B1 is the reader's matrix; B6/B6b elsewhere).
pub fn acquisition(wl: &[f64], r: &[f64], p: &CheckParams, dn: Option<&[f64]>) -> Obj {
    let CheckParams::Acquisition {
        integrity_range_nm: (lo, hi),
        b2_dn_at_least,
        b3_r_at_most,
        b4_panel_mean_range,
        b4_panel_sd_below,
        b4_empty_mean_below,
        b5_mean_r_below,
        b7_range_nm,
        b7_max_r_above,
        b8_note_above,
        b8_check_above,
        b8_fits,
        ..
    } = p
    else {
        return Obj::new();
    };
    let k: Vec<usize> = (0..wl.len())
        .filter(|&i| in_range(wl[i], *lo, *hi))
        .collect();
    let rk: Vec<f64> = k.iter().map(|&i| r[i]).collect();
    let mut out = Obj::new();
    // B2 saturation (needs raw DN)
    out.set(
        "B2",
        match dn {
            None => Obj::new()
                .with("status", "not assessed: no raw DN (spectrum input)")
                .with("outcome", V::Null),
            Some(dn) => {
                let m = nanmax(k.iter().map(|&i| dn[i]));
                Obj::new()
                    .with("status", "assessed")
                    .with("max_dn", m)
                    .with(
                        "outcome",
                        if m >= *b2_dn_at_least {
                            "Unusable"
                        } else {
                            "ok"
                        },
                    )
            }
        },
    );
    // B3 impossible reflectance
    let bad = rk.iter().any(|x| !x.is_finite() || *x <= *b3_r_at_most);
    out.set(
        "B3",
        Obj::new()
            .with("status", "assessed")
            .with("outcome", if bad { "Unusable" } else { "ok" }),
    );
    // B4 panel or empty probe
    let (mr, sd) = (np_mean(&rk), np_std(&rk, 0));
    let panel =
        b4_panel_mean_range.0 <= mr && mr <= b4_panel_mean_range.1 && sd < *b4_panel_sd_below;
    let empty = mr < *b4_empty_mean_below;
    out.set(
        "B4",
        Obj::new()
            .with("status", "assessed")
            .with("mean_R", fin(mr))
            .with("sd_R", fin(sd))
            .with("outcome", if panel || empty { "Unusable" } else { "ok" })
            .with(
                "reason",
                if panel {
                    V::from("white panel")
                } else if empty {
                    V::from("empty probe")
                } else {
                    V::Null
                },
            ),
    );
    // B5 very dark
    out.set(
        "B5",
        Obj::new()
            .with("status", "assessed")
            .with("mean_R", fin(mr))
            .with("outcome", if mr < *b5_mean_r_below { "Note" } else { "ok" }),
    );
    // B7 reflectance above 1
    let mx = npmax(
        (0..wl.len())
            .filter(|&i| in_range(wl[i], b7_range_nm.0, b7_range_nm.1))
            .map(|i| r[i]),
    );
    out.set(
        "B7",
        Obj::new()
            .with("status", "assessed")
            .with("max_R", fin(mx))
            .with("outcome", if mx > *b7_max_r_above { "Note" } else { "ok" }),
    );
    // B8 splice steps at the header joins (line fits on the SWIR1 side)
    let steps: Vec<f64> = b8_fits.iter().map(|f| b8_step(wl, r, f)).collect();
    let worst = nanmax(steps.iter().copied());
    let o = if worst > *b8_check_above {
        "Check"
    } else if worst > *b8_note_above {
        "Note"
    } else {
        "ok"
    };
    out.set(
        "B8",
        Obj::new()
            .with("status", "assessed")
            .with(
                "relative_steps",
                V::List(steps.iter().map(|s| fin(*s)).collect()),
            )
            .with("outcome", o),
    );
    out
}

fn b8_step(wl: &[f64], r: &[f64], f: &B8Fit) -> f64 {
    let (Some(a), Some(b), Some(ci), Some(di)) = (
        idx(wl, f.fit_nm.0),
        idx(wl, f.fit_nm.1),
        idx(wl, f.compare_nm),
        idx(wl, f.denominator_nm),
    ) else {
        return f64::NAN;
    };
    let pred = line_eval(&wl[a..=b], &r[a..=b], f.extrapolate_to_nm);
    let den = r[di];
    if den != 0.0 {
        (pred - r[ci]).abs() / den.abs()
    } else {
        f64::NAN
    }
}

/// Per sign: the B6b gate reason ("noise in <lo>-<hi> nm") or None (checked).
pub type Gates = BTreeMap<String, Option<String>>;

/// The reason text of a gate on a window (oracle: f"noise in {int(lo)}-{int(hi)} nm").
fn gate_reason(lo: f64, hi: f64) -> String {
    format!("noise in {}-{} nm", lo.trunc() as i64, hi.trunc() as i64)
}

/// B6b, per sign (phase 0c, DECISIONS 76): each sign is gated when N2 of its own window on the AS-MEASURED scan
/// exceeds its cut, from the table of the stream's transfer key. Returns (summary record, gates, every contaminant
/// sign gated). Summary: n2 = N2(n2_window_nm), outcome Note when any contaminant sign is gated, gated = all gated,
/// signs_gated = the gated contaminant signs.
pub fn b6b(p: &CheckParams, n2c: &mut N2Cache, key: &str) -> (Obj, Gates, bool) {
    let CheckParams::Acquisition { b6b: g, .. } = p else {
        return (Obj::new(), Gates::new(), false);
    };
    let tab = g.table(key);
    let mut gates = Gates::new();
    for s in &g.signs {
        let Some(e) = tab.get(s) else { continue };
        let (lo, hi) = e.n2_window_nm;
        let gated = n2c.get(lo, hi) > e.n2_above;
        gates.insert(s.clone(), gated.then(|| gate_reason(lo, hi)));
    }
    let v = n2c.get(g.n2_window_nm.0, g.n2_window_nm.1);
    let sg: Vec<String> = g
        .contaminant_signs
        .iter()
        .filter(|s| gates.get(*s).is_some_and(Option::is_some))
        .cloned()
        .collect();
    let all = sg.len() == g.contaminant_signs.len();
    (
        Obj::new()
            .with("status", "assessed")
            .with("n2", fin(v))
            .with("outcome", if sg.is_empty() { "ok" } else { "Note" })
            .with("gated", all)
            .with("signs_gated", super::val::strs(&sg)),
        gates,
        all,
    )
}

/// Step 2c: long-wave eligibility. Returns (record, tail unreliable for B9, tail unreliable for C1). The cut is
/// per check (phase 0c): C1 (the contaminant check only) has its own, higher cut.
pub fn longwave(p: &CheckParams, class: &str, n2c: &mut N2Cache) -> (Obj, bool, bool) {
    let CheckParams::Longwave {
        applies_to_classes,
        n2_window_nm: (lo, hi),
        tail_unreliable_if_n2_above,
        c1_tail_unreliable_if_n2_above,
        ..
    } = p
    else {
        return (Obj::new(), false, false);
    };
    let v = n2c.get(*lo, *hi);
    let applies = applies_to_classes.iter().any(|c| c == class);
    let unrel = applies && v > *tail_unreliable_if_n2_above;
    let unrel_c1 = applies && v > *c1_tail_unreliable_if_n2_above;
    (
        Obj::new()
            .with(
                "status",
                if applies {
                    "assessed"
                } else {
                    "not assessed: standard-resolution class"
                },
            )
            .with("n2", fin(v))
            .with("tail_unreliable", unrel)
            .with("tail_unreliable_c1", unrel_c1),
        unrel,
        unrel_c1,
    )
}

/// Step 5: B9 "does this look like bone?" (the truncated variant when the long-wave policy says so).
pub fn b9(
    wl: &[f64],
    r_std: &[f64],
    kernel: &Kernel,
    eps: f64,
    q: &B9Variant,
    truncated: bool,
) -> Obj {
    let e = derivative(r_std, kernel);
    let v: Vec<f64> = (0..wl.len())
        .filter(|&i| {
            q.windows_nm
                .iter()
                .any(|(lo, hi)| in_range(wl[i], *lo, *hi))
        })
        .map(|i| -e[i])
        .collect();
    if v.iter().any(|x| !x.is_finite()) {
        return Obj::new()
            .with("status", "not assessed: kernel support")
            .with("score", V::Null)
            .with("outcome", V::Null);
    }
    let m = np_mean(&v);
    let sd = np_std(&v, 0) + eps;
    let z: Vec<f64> = v.iter().map(|x| (x - m) / sd).collect();
    let n = z.len() as f64;
    let rows = q.prototypes.shape.first().copied().unwrap_or(0);
    let mut s = f64::NEG_INFINITY;
    for k in 0..rows {
        let p = q.prototypes.row(k).unwrap_or(&[]);
        if p.len() != z.len() {
            return Obj::new()
                .with("status", "not assessed: prototype length")
                .with("score", V::Null)
                .with("outcome", V::Null);
        }
        let mut d = 0.0;
        for (a, b) in z.iter().zip(p) {
            d += a * b;
        }
        s = s.max(d / n);
    }
    Obj::new()
        .with(
            "status",
            if truncated {
                "assessed: truncated (long-wave region too noisy)"
            } else {
                "assessed"
            },
        )
        .with("score", s)
        .with(
            "outcome",
            if s < q.fail_if_score_below {
                "fail"
            } else {
                "pass"
            },
        )
}

/// r_b per band (a band with a projection is read on the projected E: the OH-corrected 1545 nm trough).
fn readings(wl: &[f64], e: &[f64], bands: &Bands, ids: &[String]) -> BTreeMap<String, f64> {
    ids.iter()
        .map(|b| (b.clone(), band_value(e, wl, bands, b)))
        .collect()
}

/// u_sign of a band (-1 for a trough; default +1).
fn u_sign(bands: &Bands, id: &str) -> f64 {
    bands.bands.get(id).map_or(1.0, |b| b.u_sign)
}

fn weight(bands: &Bands, id: &str) -> f64 {
    bands
        .bands
        .get(id)
        .and_then(|b| b.weight)
        .unwrap_or(f64::NAN)
}

/// Step 7: the organic-evidence level (normative rule; frozen in A3). Returns (record, level, S).
pub fn evidence(
    wl: &[f64],
    e: &[f64],
    p: &CheckParams,
    bands: &Bands,
    sd_e: &BTreeMap<String, f64>,
) -> (Obj, String, Option<f64>) {
    let CheckParams::EvidenceLevels {
        core_bands: core,
        extra_band: extra,
        extra_band_faint_u,
        sd_max_u,
        unreadable_only_if_u_below,
        clear_u,
        strong_u,
        guard_window_nm,
        guard_max_e_below,
        must_be_readable,
        min_readable_core,
        extra_band_max_u,
        nh_type_bands,
        nh_type_min_u,
        clear_min_lit_core,
        ..
    } = p
    else {
        return (Obj::new(), String::new(), None);
    };
    let mut ids: Vec<String> = core.clone();
    ids.push(extra.clone());
    let r = readings(wl, e, bands, &ids);
    let guard0 = window_max(e, wl, guard_window_nm.0, guard_window_nm.1);
    if r.values().any(|x| !x.is_finite()) || !guard0.is_finite() {
        return (
            Obj::new()
                .with("status", "not assessed: non-finite band reading")
                .with("level", V::Null)
                .with("S", V::Null)
                .with("n_readable_core", V::Null)
                .with("n_lit_core", V::Null)
                .with("guard_max_E", V::Null)
                .with("bands", Obj::new()),
            String::new(),
            None,
        );
    }
    let w: BTreeMap<&String, f64> = ids.iter().map(|b| (b, weight(bands, b))).collect();
    let u: BTreeMap<&String, f64> = ids
        .iter()
        .map(|b| (b, u_sign(bands, b) * r[b] / w[b]))
        .collect();
    let sdu: BTreeMap<&String, f64> = ids
        .iter()
        .map(|b| (b, sd_e.get(b).map_or(f64::NAN, |s| s / w[b])))
        .collect();
    let state = |b: &String, faint: f64| -> &'static str {
        if sdu[b] > *sd_max_u && u[b] < *unreadable_only_if_u_below {
            "can't tell"
        } else if u[b] >= *strong_u {
            "strong"
        } else if u[b] >= *clear_u {
            "clear"
        } else if u[b] > faint {
            "faint"
        } else {
            "flat"
        }
    };
    let mut st: BTreeMap<&String, &str> = BTreeMap::new();
    for b in core {
        let faint = bands
            .bands
            .get(b)
            .and_then(|x| x.faint_u)
            .unwrap_or(f64::NAN);
        st.insert(b, state(b, faint));
    }
    st.insert(extra, state(extra, *extra_band_faint_u));
    let readable = |b: &String| st.get(b).is_some_and(|s| *s != "can't tell");
    let ru: Vec<f64> = core.iter().filter(|b| readable(b)).map(|b| u[b]).collect();
    let s_val = if ru.is_empty() {
        None
    } else {
        Some(crate::n2::median(&ru))
    };
    let nread = core.iter().filter(|b| readable(b)).count();
    let allflat = core.iter().all(|b| matches!(st[b], "flat" | "can't tell"));
    // Step 7: unreadable ("can't tell") only if SD_b > sd_max AND u_b < unreadable_only_if_u_below (Codex
    // Phase 3 review 4; the frozen research script used the SD condition alone for N-H 2044)
    let nh_unreadable = st[extra] == "can't tell";
    let nh_ok = u[extra] <= *extra_band_max_u || nh_unreadable;
    let guard_val = window_max(e, wl, guard_window_nm.0, guard_window_nm.1);
    let guard_ok = guard_val < *guard_max_e_below;
    let none = allflat
        && must_be_readable
            .iter()
            .all(|b| core.contains(b) && readable(b))
        && nread as u64 >= *min_readable_core
        && nh_ok
        && guard_ok;
    let lit = core
        .iter()
        .filter(|b| matches!(st[*b], "faint" | "clear" | "strong"))
        .count();
    let mut nh_vals = Vec::new();
    for b in nh_type_bands {
        if b == extra {
            if !nh_unreadable {
                nh_vals.push(u[b]);
            }
        } else if core.contains(b) && readable(b) {
            nh_vals.push(u[b]);
        }
    }
    // Python max(): the first maximal element; NaN never wins a comparison
    let nh_type = nh_vals
        .iter()
        .copied()
        .reduce(|a, b| if b > a { b } else { a });
    let nh_ok_lev = nh_type.is_some_and(|x| x >= *nh_type_min_u);
    let strong = s_val.is_some_and(|s| s >= *strong_u) && lit == nread && nh_ok_lev;
    let clear =
        s_val.is_some_and(|s| s >= *clear_u) && lit as u64 >= *clear_min_lit_core && nh_ok_lev;
    let nh_lit = u[extra] > 0.0 && !nh_unreadable;
    let cant = !none && lit == 0 && allflat && guard_ok && !nh_lit;
    let level = if none {
        "none"
    } else if cant {
        "can't tell"
    } else if strong {
        "strong"
    } else if clear {
        "clear"
    } else {
        "trace"
    };
    let mut bo = Obj::new();
    for b in &ids {
        bo.set(
            b,
            Obj::new()
                .with("E", r[b])
                .with("u", u[b])
                .with("sd_u", fin(sdu[b]))
                .with("state", st[b]),
        );
    }
    let rec = Obj::new()
        .with("status", "assessed")
        .with("level", level)
        .with("S", s_val)
        .with("n_readable_core", nread)
        .with("n_lit_core", lit)
        .with("guard_max_E", guard_val)
        .with("bands", bo);
    (rec, level.to_string(), s_val)
}

/// Step 9: the ZooMS band pattern (A17). Returns (record, ZooMS verdict after the 1545 vote, band-pattern verdict,
/// pattern).
pub fn zooms(
    wl: &[f64],
    e: &[f64],
    p: &CheckParams,
    bands: &Bands,
    sd_e: &BTreeMap<String, f64>,
) -> (Obj, String, String, String) {
    let CheckParams::ZoomsPatterns {
        protein_bands: prot,
        ch_bands: ch,
        lit_if_e_above,
        sd_max_frac_w,
        unreadable_only_if_e_below_frac_w,
        vote,
        ..
    } = p
    else {
        return (Obj::new(), String::new(), String::new(), String::new());
    };
    let six: Vec<String> = prot.iter().chain(ch).cloned().collect();
    let r = readings(wl, e, bands, &six);
    if r.values().any(|x| !x.is_finite()) {
        return (
            Obj::new()
                .with("status", "not assessed: non-finite band reading")
                .with("pattern", V::Null)
                .with("verdict", "Can't tell")
                .with("pattern_verdict", "Can't tell")
                .with("bands", Obj::new())
                .with("vote_1545", false),
            "Can't tell".to_string(),
            "Can't tell".to_string(),
            String::new(),
        );
    }
    let mut rd = BTreeMap::new();
    let mut lit = BTreeMap::new();
    for b in &six {
        let wb = weight(bands, b);
        let sd = sd_e.get(b).copied().unwrap_or(f64::NAN);
        rd.insert(
            b.clone(),
            !(sd > sd_max_frac_w * wb && r[b] < unreadable_only_if_e_below_frac_w * wb),
        );
        lit.insert(b.clone(), r[b] > *lit_if_e_above);
    }
    let np = prot.iter().filter(|b| lit[*b]).count();
    let nc = ch.iter().filter(|b| lit[*b]).count();
    let pat = if np + nc == 0 {
        "A"
    } else if np == 0 {
        "B"
    } else if np == 1 {
        "C"
    } else if nc < ch.len() {
        "D"
    } else {
        "E"
    };
    let p_rd = prot.iter().all(|b| rd[b]);
    let p_flat = np == 0;
    let good = six.iter().all(|b| rd[b] && lit[b]);
    let v = if p_rd && p_flat {
        "Unlikely"
    } else if good {
        "Good"
    } else if p_rd && !p_flat && (np == 1 || ch.iter().any(|b| !lit[b] && rd[b])) {
        "Borderline"
    } else {
        "Can't tell"
    };
    let mut bo = Obj::new();
    for b in &six {
        bo.set(
            b,
            Obj::new()
                .with("E", r[b])
                .with("lit", lit[b])
                .with("readable", rd[b])
                .with("sd_E", fin(sd_e.get(b).copied().unwrap_or(f64::NAN))),
        );
    }
    // verdict = the ZooMS verdict (after the 1545 vote); pattern_verdict = the band pattern alone, which the
    // radiocarbon / isotopes rules read (+D, lift, positive signs): the vote belongs to ZooMS only (DECISIONS 75)
    let mut out = Obj::new()
        .with("status", "assessed")
        .with("pattern", pat)
        .with("verdict", v)
        .with("pattern_verdict", v)
        .with("bands", bo);
    // phase 0c vote (DECISIONS 75): a readable, lit OH-corrected 1545 nm trough turns Unlikely into Borderline
    let mut verdict = v.to_string();
    let mut voted = false;
    if let Some(vt) = vote {
        let b = &vt.band;
        let rv = band_value(e, wl, bands, b);
        let wv = weight(bands, b);
        let uv = u_sign(bands, b) * rv / wv;
        let sdu = sd_e.get(b).map_or(f64::NAN, |s| s / wv);
        let finite = rv.is_finite();
        let rdv = finite && !(sdu > *sd_max_frac_w && uv < *unreadable_only_if_e_below_frac_w);
        let litv = finite && uv > vt.lit_if_u_above;
        voted = v == vt.from_verdict && rdv && litv;
        if voted {
            verdict = vt.to_verdict.clone();
            out.set("verdict", verdict.as_str());
        }
        out.set(
            "vote_band",
            Obj::new()
                .with("id", b.as_str())
                .with("E", fin(rv))
                .with("u", fin(uv))
                .with("sd_u", fin(sdu))
                .with("readable", rdv)
                .with("lit", litv),
        );
    }
    out.set("vote_1545", voted);
    (out, verdict, v.to_string(), pat.to_string())
}

fn sharp_of(n: &[f64], wl: &[f64], s: &Sharp) -> f64 {
    sharp(
        n,
        wl,
        s.centre_nm,
        s.half_width as f64,
        s.flank_from as f64,
        s.flank_to as f64,
    )
}

/// Heat (burnt): charred by the visible edge alone; calcined. `n17` = the signs-kernel derivative.
pub fn heat(wl: &[f64], r_std: &[f64], n17: &[f64], p: &CheckParams) -> Obj {
    let CheckParams::Heat {
        edge50_scan_from_nm,
        edge50_reference_nm,
        edge50_fraction,
        edge50_at_least_nm,
        oh1433,
        oh979,
        r_vis_range_nm,
        r_vis_at_least,
        ..
    } = p
    else {
        return Obj::new();
    };
    let reference = window_mean(r_std, wl, edge50_reference_nm.0, edge50_reference_nm.1);
    let edge50 = idx(wl, *edge50_scan_from_nm).and_then(|a| {
        (a..r_std.len())
            .find(|&i| r_std[i] / reference >= *edge50_fraction)
            .map(|i| wl[i])
    });
    let charred = edge50.is_some_and(|x| x >= *edge50_at_least_nm);
    let o1433 = sharp_of(n17, wl, &oh1433.0);
    let o979 = sharp_of(n17, wl, &oh979.0);
    let rvis = window_mean(r_std, wl, r_vis_range_nm.0, r_vis_range_nm.1);
    if [reference, o1433, o979, rvis]
        .iter()
        .any(|x| !x.is_finite())
    {
        // Codex Phase 3 review 3: a non-finite input is "not assessed", never "not burnt"
        return Obj::new()
            .with("status", NOT_FINITE)
            .with("edge50_nm", edge50)
            .with("OH1433", fin(o1433))
            .with("OH979", fin(o979))
            .with("R_vis", fin(rvis))
            .with("charred", V::Null)
            .with("calcined", V::Null)
            .with("fired", V::Null);
    }
    let calc = !charred && o1433 >= oh1433.1 && o979 >= oh979.1 && rvis >= *r_vis_at_least;
    Obj::new()
        .with("edge50_nm", edge50)
        .with("OH1433", o1433)
        .with("OH979", o979)
        .with("R_vis", rvis)
        .with("charred", charred)
        .with("calcined", calc)
        .with("fired", charred || calc)
}

fn gated_rec(reason: &str) -> Obj {
    Obj::new()
        .with("status", format!("gated: {reason}"))
        .with("fired", V::Null)
        .with("gated", true)
}

/// The four signs (plaster, wax, ester, burnt; no clay). `gates`: per sign, the B6b gate reason (that sign is
/// "gated: <reason>" and not read) or None. Every sign record ends with `gated` (bool).
pub fn signs(
    wl: &[f64],
    r_std: &[f64],
    p: &CheckParams,
    heat_p: &CheckParams,
    gates: &Gates,
) -> Obj {
    let names = ["plaster", "wax", "ester", "burnt"];
    let reason = |n: &str| gates.get(n).and_then(|g| g.as_deref());
    let mut out = Obj::new();
    if names.iter().all(|n| reason(n).is_some()) {
        for n in names {
            out.set(n, gated_rec(reason(n).unwrap_or_default()));
        }
        return out;
    }
    let CheckParams::Signs {
        kernel,
        plaster_bands,
        plaster_fires_if_joint_load_above,
        wax_all_of,
        ester,
        ..
    } = p
    else {
        return out;
    };
    let n = derivative(r_std, kernel);
    let loads: Vec<f64> = plaster_bands
        .iter()
        .map(|(s, med, contrast)| (sharp_of(&n, wl, s) - med) / contrast)
        .collect();
    // Python min(): the first minimal element
    let jl = loads
        .iter()
        .copied()
        .reduce(|a, b| if b < a { b } else { a })
        .unwrap_or(f64::NAN);
    // Codex Phase 3 review 3: a non-finite reading is "not assessed", never "not detected"
    out.set(
        "plaster",
        if loads.iter().any(|x| !x.is_finite()) {
            Obj::new()
                .with("status", NOT_FINITE)
                .with("joint_load", V::Null)
                .with("fired", V::Null)
        } else {
            Obj::new()
                .with("status", "assessed")
                .with("joint_load", jl)
                .with("fired", jl > *plaster_fires_if_joint_load_above)
        },
    );
    let mut stats = Obj::new();
    let mut fired = true;
    let mut finite = true;
    for (s, at_least) in wax_all_of {
        let v = sharp_of(&n, wl, s);
        finite &= v.is_finite();
        stats.set(&s.centre_key, v);
        fired &= v >= *at_least;
    }
    out.set(
        "wax",
        if finite {
            Obj::new()
                .with("status", "assessed")
                .with("sharp", stats)
                .with("fired", fired)
        } else {
            let mut st = Obj::new();
            for (k, v) in &stats.0 {
                st.set(k, v.as_f64().map_or(V::Null, fin));
            }
            Obj::new()
                .with("status", NOT_FINITE)
                .with("sharp", st)
                .with("fired", V::Null)
        },
    );
    let v = sharp_of(&n, wl, &ester.0);
    out.set(
        "ester",
        if v.is_finite() {
            Obj::new()
                .with("status", "assessed")
                .with("sharp", v)
                .with("fired", v >= ester.1)
        } else {
            Obj::new()
                .with("status", NOT_FINITE)
                .with("sharp", V::Null)
                .with("fired", V::Null)
        },
    );
    let mut b = Obj::new().with("status", "assessed");
    b.update(heat(wl, r_std, &n, heat_p));
    out.set("burnt", b);
    for nm in names {
        if let Some(r) = reason(nm) {
            out.set(nm, gated_rec(r));
        } else if let Some(V::Map(o)) = out.get_mut(nm) {
            o.set("gated", false);
        }
    }
    out
}

/// The "possible thin coating" tier (phase 0c, DECISIONS 74/76): a note only. Fires when no hard contaminant sign
/// fired, the scan is eligible (a class in always_classes, else N2(otherwise window) <= otherwise_n2_at_most) and a
/// CHECKED sign's statistic reaches its soft line (C1 / C1t excess, ester sharp, plaster joint load).
pub fn soft_tier(
    ps: &CheckParams,
    pc1: &CheckParams,
    class: &str,
    n2c: &mut N2Cache,
    sg: &Obj,
    c1r: &Obj,
) -> Obj {
    let none = || {
        Obj::new()
            .with("status", "not assessed: no soft tier")
            .with("eligible", false)
            .with("fired", false)
            .with("components", V::List(vec![]))
    };
    let CheckParams::Signs {
        soft_tier: Some(st),
        ..
    } = ps
    else {
        return none();
    };
    let sign = |n: &str| -> Option<&Obj> {
        if n == "C1" {
            Some(c1r)
        } else {
            sg.get(n).and_then(V::as_obj)
        }
    };
    let fired = |o: Option<&Obj>| o.and_then(|o| o.get("fired")).is_some_and(V::truthy);
    let hard = st.requires_no_hard_sign.iter().any(|n| fired(sign(n)));
    let (lo, hi) = st.otherwise_n2_window_nm;
    let eligible =
        st.always_classes.iter().any(|c| c == class) || n2c.get(lo, hi) <= st.otherwise_n2_at_most;
    let num = |o: Option<&Obj>, k: &str| o.and_then(|o| o.get(k)).and_then(V::as_f64);
    let status = |o: Option<&Obj>| {
        o.and_then(|o| o.get("status"))
            .and_then(V::as_str)
            .unwrap_or("")
            .to_string()
    };
    let mut comps: Vec<String> = Vec::new();
    if eligible {
        let (full, tr) = match pc1 {
            CheckParams::C1 {
                full, truncated, ..
            } => (
                full.soft_excess_at_least,
                truncated.as_ref().and_then(|t| t.soft_excess_at_least),
            ),
            _ => (None, None),
        };
        let c1s = status(Some(c1r));
        let ex = num(Some(c1r), "excess");
        if c1s == "assessed" && full.is_some_and(|t| ex.is_some_and(|x| x >= t)) {
            comps.push("C1".into());
        } else if c1s.starts_with("assessed: truncated")
            && tr.is_some_and(|t| ex.is_some_and(|x| x >= t))
        {
            comps.push("C1t".into());
        }
        let es = sign("ester");
        if status(es) == "assessed" && num(es, "sharp").is_some_and(|x| x >= st.ester_at_least) {
            comps.push("ester".into());
        }
        let pl = sign("plaster");
        if status(pl) == "assessed"
            && num(pl, "joint_load").is_some_and(|x| x >= st.plaster_joint_load_at_least)
        {
            comps.push("plaster".into());
        }
    }
    let fires = eligible && !hard && !comps.is_empty();
    Obj::new()
        .with(
            "status",
            if eligible {
                "assessed".to_string()
            } else {
                format!("not assessed: {}", gate_reason(lo, hi))
            },
        )
        .with("eligible", eligible)
        .with("fired", fires)
        .with(
            "components",
            super::val::strs(if fires { &comps[..] } else { &[] }),
        )
}

/// C1: generic foreign organic. `gate`: C1's B6b gate reason (None = checked). `tail_unreliable`: the long-wave
/// tail is unreliable FOR C1. `longwave_mode`: "truncated" or "not_assessed" then. The record ends with `gated`.
pub fn c1(
    wl: &[f64],
    e: &[f64],
    p: &CheckParams,
    gate: Option<&str>,
    specific_fired: bool,
    tail_unreliable: bool,
    longwave_mode: &str,
) -> Obj {
    let none = |s: &str| {
        Obj::new()
            .with("status", s)
            .with("excess", V::Null)
            .with("fired", V::Null)
            .with("gated", false)
    };
    if let Some(g) = gate {
        return Obj::new()
            .with("status", format!("gated: {g}"))
            .with("excess", V::Null)
            .with("fired", V::Null)
            .with("gated", true);
    }
    if specific_fired {
        return none("skipped: a specific sign fired");
    }
    let CheckParams::C1 {
        read_half_width_nm,
        full,
        truncated,
        ..
    } = p
    else {
        return none("not assessed: no C1 parameters");
    };
    let (q, st): (&C1Variant, &str) = if tail_unreliable {
        match (longwave_mode, truncated) {
            ("truncated", Some(t)) => (t, "assessed: truncated (long-wave region too noisy)"),
            _ => return none("not assessed: long-wave region too noisy"),
        }
    } else {
        (full, "assessed")
    };
    let hw = *read_half_width_nm as f64;
    let mut chs = 0.0;
    for c in &q.ch_bands_nm {
        chs += band_reading(e, wl, *c, hw);
    }
    let mut nhs = 0.0;
    for c in &q.nh_bands_nm {
        nhs += band_reading(e, wl, *c, hw);
    }
    let ex = chs - (q.a + q.b * nhs);
    if !ex.is_finite() {
        return none("not assessed: kernel support");
    }
    Obj::new()
        .with("status", st)
        .with("excess", ex)
        .with("fired", ex > q.fires_if_excess_above)
        .with("gated", false)
}
