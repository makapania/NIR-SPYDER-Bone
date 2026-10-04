//! Golden runner for engine-check parameter files and analysis profiles (port of
//! `reference/oracle/checkgold.py`; PLAN section 4: goldens run on load and must test something: >= 3 cases,
//! every case a non-empty `expected`, tolerance <= 1e-9 abs and rel, finite numbers, zero-comparison runs fail;
//! labels, booleans and null exactly).
//!
//! Check case: {name, spectrum_sha256 (the standard-resolution stream, or the as-measured spectrum for
//! acquisition and longwave), inputs, expected {flat dotted keys}}. Profile case: {name, inputs, expected}.

use std::collections::BTreeMap;

use serde_json::Value;

use super::checks::{self as C, N2Cache};
use super::kernels::derivative;
use super::val::{flatten, matches_check_expected, Obj, V};
use super::verdict::{verdict_for, VerdictInputs};
use crate::plugins::checks::{CheckParams, EngineCheck};
use crate::plugins::golden::{cases, tolerance, Failure, GoldenRun, GoldenSpectra};
use crate::plugins::profiles::AnalysisProfile;
use crate::plugins::tables::Bands;
use crate::plugins::{Node, PResult, PluginError};

/// What the check goldens need from the other plug-ins.
pub struct CheckEnv<'a> {
    pub bands: Option<&'a Bands>,
    /// check name -> its parameters (signs and heat need each other).
    pub checks: BTreeMap<String, &'a CheckParams>,
}

fn flat_map(o: Obj) -> BTreeMap<String, V> {
    flatten(&V::Map(o)).into_iter().collect()
}

/// An optional string input (null or missing = None).
fn input_opt_str(inputs: &Node, k: &str) -> PResult<Option<String>> {
    match inputs.opt(k) {
        None => Ok(None),
        Some(n) if n.v.is_null() => Ok(None),
        Some(n) => Ok(Some(n.str()?.to_string())),
    }
}

fn input_bool(inputs: &Node, k: &str) -> PResult<bool> {
    match inputs.opt(k) {
        None => Ok(false),
        Some(b) => b.bool(),
    }
}

/// The check's outputs for one spectrum, flattened (what the goldens compare).
pub fn compute_check(
    c: &EngineCheck,
    env: &CheckEnv,
    wl: &[f64],
    x: &[f64],
    inputs: &Node,
) -> PResult<BTreeMap<String, V>> {
    let p = &c.params;
    let need = |name: &str| -> PResult<&CheckParams> {
        env.checks.get(name).copied().ok_or_else(|| {
            PluginError::new(
                crate::plugins::ErrorKind::CrossFile,
                format!("check {} needs the '{name}' check file", c.check),
            )
        })
    };
    let bands = || {
        env.bands.ok_or_else(|| {
            PluginError::new(crate::plugins::ErrorKind::CrossFile, "no loaded band table")
        })
    };
    Ok(match p {
        CheckParams::Acquisition { .. } => {
            let mut n2c = N2Cache::new(wl, x);
            let mut r = C::acquisition(wl, x, p, None);
            let key = input_opt_str(inputs, "transfer_key")?.unwrap_or_else(|| "none".into());
            r.set("B6b", C::b6b(p, &mut n2c, &key).0);
            flat_map(r)
        }
        CheckParams::Longwave { .. } => {
            let cls = inputs.req("instrument_class")?.str()?;
            flat_map(C::longwave(p, cls, &mut N2Cache::new(wl, x)).0)
        }
        CheckParams::B9 {
            kernel,
            std_epsilon,
            full,
            truncated,
        } => {
            let tr = input_bool(inputs, "truncated")?;
            let q = if tr {
                truncated.as_ref().ok_or_else(|| {
                    PluginError::golden("a truncated B9 case needs the 'truncated' block")
                })?
            } else {
                full
            };
            flat_map(C::b9(wl, x, kernel, *std_epsilon, q, tr))
        }
        CheckParams::EvidenceLevels { kernel, .. } | CheckParams::ZoomsPatterns { kernel, .. } => {
            let e = derivative(x, kernel);
            let mut sd = BTreeMap::new();
            if let Some(s) = inputs.opt("sd_E") {
                for (b, v) in s.entries()? {
                    sd.insert(b, v.f64()?);
                }
            }
            if matches!(p, CheckParams::EvidenceLevels { .. }) {
                flat_map(C::evidence(wl, &e, p, bands()?, &sd).0)
            } else {
                flat_map(C::zooms(wl, &e, p, bands()?, &sd).0)
            }
        }
        CheckParams::C1 { kernel, .. } => {
            let e = derivative(x, kernel);
            let mode = inputs
                .opt("longwave_mode")
                .map(|m| m.str().map(str::to_string))
                .transpose()?
                .unwrap_or_else(|| "not_assessed".into());
            let gate = input_opt_str(inputs, "gate")?;
            flat_map(C::c1(
                wl,
                &e,
                p,
                gate.as_deref(),
                input_bool(inputs, "specific_fired")?,
                input_bool(inputs, "tail_unreliable")?,
                &mode,
            ))
        }
        CheckParams::Signs { .. } => {
            let mut gates = C::Gates::new();
            if let Some(g) = inputs.opt("gates") {
                for (k, v) in g.entries()? {
                    let r = if v.v.is_null() {
                        None
                    } else {
                        Some(v.str()?.to_string())
                    };
                    gates.insert(k, r);
                }
            }
            flat_map(C::signs(wl, x, p, need("heat")?, &gates))
        }
        CheckParams::Heat { .. } => {
            let CheckParams::Signs { kernel, .. } = need("signs")? else {
                return Err(PluginError::golden(
                    "the heat goldens need the signs kernel",
                ));
            };
            let n = derivative(x, kernel);
            flat_map(C::heat(wl, x, &n, p))
        }
    })
}

/// A profile's outputs for one case's inputs: verdict, rule_step, model_verdict, notes (comma-joined keys).
pub fn compute_profile(prof: &AnalysisProfile, inputs: &Node) -> PResult<BTreeMap<String, V>> {
    let fired: Vec<String> = inputs.req("signs_fired")?.vec_str()?;
    let gated = input_bool(inputs, "gated")?;
    let signs_gated: Vec<String> = match inputs.opt("signs_gated") {
        None => Vec::new(),
        Some(g) => g.vec_str()?,
    };
    let mut sg = Obj::new();
    for s in ["plaster", "wax", "ester", "burnt"] {
        sg.set(
            s,
            if gated || signs_gated.iter().any(|g| g == s) {
                Obj::new().with("fired", V::Null).with("status", "gated")
            } else {
                Obj::new()
                    .with("fired", fired.iter().any(|f| f == s))
                    .with("status", "assessed")
            },
        );
    }
    if let Some(sc) = inputs.opt("soft_components") {
        let comps = sc.vec_str()?;
        if !comps.is_empty() {
            sg.set(
                "soft",
                Obj::new()
                    .with("fired", true)
                    .with("components", super::val::strs(&comps)),
            );
        }
    }
    let c1 = Obj::new()
        .with("status", inputs.req("c1_status")?.str()?)
        .with("fired", fired.iter().any(|f| f == "C1"));
    let m = match inputs.opt("m") {
        None => None,
        Some(x) => Some(x.f64()?),
    };
    let comps: Vec<(String, Option<f64>)> = match inputs.opt("components") {
        None => Vec::new(),
        Some(c) => c
            .entries()?
            .into_iter()
            .map(|(k, v)| Ok((k, if v.v.is_null() { None } else { Some(v.f64()?) })))
            .collect::<PResult<_>>()?,
    };
    let level = inputs.req("evidence_level")?.str()?;
    // zooms_verdict: the band-pattern verdict; zooms_shown_verdict: the ZooMS verdict after the 1545 nm vote
    let zv = inputs.req("zooms_verdict")?.str()?;
    let zs = match inputs.opt("zooms_shown_verdict") {
        None => zv,
        Some(z) => z.str()?,
    };
    let zp = inputs.req("zooms_pattern")?.str()?;
    // n_type_lit: the ZooMS-check bands lit and readable (the protein line's strength test)
    let mut zb = Obj::new();
    if let Some(l) = inputs.opt("n_type_lit") {
        for b in l.vec_str()? {
            zb.set(&b, Obj::new().with("lit", true).with("readable", true));
        }
    }
    let mut zrec = Obj::new().with("bands", zb);
    // zooms_check: the ZooMS check record as production writes it (bands lit / readable, vote_band); overrides
    // n_type_lit
    if let Some(zc) = inputs.opt("zooms_check") {
        let flags = |n: &Node| -> PResult<Obj> {
            Ok(Obj::new()
                .with("lit", n.req("lit")?.bool()?)
                .with("readable", n.req("readable")?.bool()?))
        };
        let mut bands = Obj::new();
        if let Some(bs) = zc.opt("bands") {
            for (b, n) in bs.entries()? {
                bands.set(&b, flags(&n)?);
            }
        }
        zrec = Obj::new().with("bands", bands);
        if let Some(vb) = zc.opt("vote_band") {
            zrec.set("vote_band", flags(&vb)?.with("id", vb.req("id")?.str()?));
        }
    }
    // ryder2045: the published Ryder 2045 reading (the Ryder ZooMS line)
    let mut models = Obj::new();
    if let Some(r) = inputs.opt("ryder2045") {
        models.set("ryder2045", Obj::new().with("value", r.f64()?));
    }
    let x = VerdictInputs {
        signs: &sg,
        c1: &c1,
        level,
        zooms_verdict: zv,
        zooms_shown_verdict: zs,
        zooms_pattern: zp,
        zooms_rec: &zrec,
        models: &models,
        gated,
    };
    let r = verdict_for(prof, m, &comps, &x);
    let keys = |k: &str, field: Option<&str>| -> String {
        match r.get(k) {
            Some(V::List(l)) => l
                .iter()
                .filter_map(|e| match field {
                    None => e.as_str().map(str::to_string),
                    Some(f) => e
                        .as_obj()
                        .and_then(|o| o.get(f))
                        .and_then(V::as_str)
                        .map(str::to_string),
                })
                .collect::<Vec<_>>()
                .join(","),
            _ => String::new(),
        }
    };
    let mut out = BTreeMap::new();
    out.insert(
        "verdict".into(),
        r.get("verdict").cloned().unwrap_or(V::Null),
    );
    out.insert(
        "rule_step".into(),
        r.get("rule_step").cloned().unwrap_or(V::Null),
    );
    out.insert(
        "model_verdict".into(),
        r.get("model_verdict").cloned().unwrap_or(V::Null),
    );
    out.insert("notes_shown".into(), V::Str(keys("notes_shown", None)));
    out.insert("notes_all".into(), V::Str(keys("notes_all", Some("key"))));
    out.insert("flags".into(), V::Str(keys("flags", Some("key"))));
    Ok(out)
}

fn compare(
    run: &mut GoldenRun,
    case: &str,
    exp: &[(String, Node)],
    got: &BTreeMap<String, V>,
) -> PResult<()> {
    let tol = run.tolerance;
    let n0 = run.checks;
    for (k, w) in exp {
        if let Value::Number(n) = w.v {
            if !n.as_f64().is_some_and(f64::is_finite) {
                return Err(PluginError::golden(format!(
                    "case {case}: expected {k} must be a finite number, a label, a bool or null"
                )));
            }
        } else if !matches!(w.v, Value::String(_) | Value::Bool(_) | Value::Null) {
            return Err(PluginError::golden(format!(
                "case {case}: expected {k} must be a finite number, a label, a bool or null"
            )));
        }
        match got.get(k) {
            None => {
                run.failures.push(Failure {
                    case: case.into(),
                    key: format!("{k} (not produced)"),
                    got: "none".into(),
                    want: w.v.to_string(),
                });
                run.checks += 1;
            }
            Some(g) => {
                if matches_check_expected(g, w.v, tol.abs, tol.rel) {
                    if let (Some(gv), Some(wv)) = (g.as_f64(), w.v.as_f64()) {
                        run.worst_ratio = run.worst_ratio.max(tol.ratio(gv, wv));
                    }
                    run.checks += 1;
                } else {
                    run.failures.push(Failure {
                        case: case.into(),
                        key: k.clone(),
                        got: super::val::py_json(g, false),
                        want: w.v.to_string(),
                    });
                    run.checks += 1;
                }
            }
        }
    }
    if run.checks == n0 {
        return Err(PluginError::golden(format!(
            "case {case}: zero comparisons"
        )));
    }
    Ok(())
}

fn case_parts<'a>(c: &Node<'a>, i: usize) -> PResult<(String, Vec<(String, Node<'a>)>)> {
    let name = c
        .opt("name")
        .and_then(|n| n.v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("case {i}"));
    let exp = match c.opt("expected") {
        Some(e) if e.v.is_object() => e.entries()?,
        _ => Vec::new(),
    };
    if exp.is_empty() {
        return Err(PluginError::golden(format!(
            "case {name}: empty expected block"
        )));
    }
    Ok((name, exp))
}

/// Run every golden case of an engine-check file. `spectra`: the golden spectra file named by the block.
pub fn run_check_goldens(
    c: &EngineCheck,
    doc: &Node,
    env: &CheckEnv,
    spectra: &GoldenSpectra,
) -> PResult<GoldenRun> {
    let golden = doc.req("golden")?;
    let mut run = GoldenRun::empty(tolerance(&golden)?);
    for (i, case) in cases(&golden)?.iter().enumerate() {
        let (name, exp) = case_parts(case, i)?;
        let sha = case
            .opt("spectrum_sha256")
            .and_then(|s| s.v.as_str())
            .unwrap_or("");
        let g = spectra.spectra.get(sha).ok_or_else(|| {
            PluginError::golden(format!(
                "case {name}: spectrum not in the golden spectra file"
            ))
        })?;
        let empty = Value::Object(Default::default());
        let inputs = case.opt("inputs").unwrap_or(Node::root(&empty));
        let got = compute_check(c, env, &g.wl, &g.x, &inputs)?;
        compare(&mut run, &name, &exp, &got)?;
    }
    Ok(run)
}

/// Run every golden case of an analysis profile (no spectra).
pub fn run_profile_goldens(p: &AnalysisProfile, doc: &Node) -> PResult<GoldenRun> {
    let golden = doc.req("golden")?;
    let mut run = GoldenRun::empty(tolerance(&golden)?);
    for (i, case) in cases(&golden)?.iter().enumerate() {
        let (name, exp) = case_parts(case, i)?;
        let got = compute_profile(p, &case.req("inputs")?)?;
        compare(&mut run, &name, &exp, &got)?;
    }
    Ok(run)
}
