//! Golden spectra files and the hardened golden runner (PLAN section 4; Phase 0a A10; spyder_ref
//! `run_goldens`). Goldens run on load and must test something:
//!
//! * the engine owns the tolerance: at most 1e-9 abs and 1e-9 rel; a file may tighten it, never loosen it;
//! * at least 3 cases; every case a non-empty `expected` block with the outputs its kind requires and no output
//!   the engine does not know; expected values finite;
//! * regression files: at least one case carries the intermediate feature vector (of the declared length);
//! * a case or a whole run with zero comparisons is an error;
//! * output mismatches, wrong dimensions and non-finite outputs are failing comparisons.
//!
//! A comparison passes when |got - want| <= abs + rel * |want|; vectors element-wise.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::json::{read_json_file, resolve_sidecars};
use super::{
    spectrum_sha256, ErrorKind, Node, PResult, PluginError, ScanContext, GOLDEN_SPECTRA_FORMAT,
};
use crate::model::{Model, ModelKind};
use crate::preprocess::Spectrum;
use crate::transfer::Transfer;

/// The engine's maximum golden tolerance (abs and rel), for every kind in engine 1.1.
pub const GOLDEN_MAX_TOL: f64 = 1e-9;
/// Minimum number of golden cases per file.
pub const GOLDEN_MIN_CASES: usize = 3;

/// One golden spectrum.
#[derive(Debug, Clone)]
pub struct GoldenSpectrum {
    pub name: String,
    pub wl: Vec<f64>,
    pub x: Vec<f64>,
}

/// A golden spectra file, keyed by spectrum hash.
#[derive(Debug, Clone)]
pub struct GoldenSpectra {
    pub id: String,
    pub file_sha256: String,
    pub spectra: BTreeMap<String, GoldenSpectrum>,
}

/// Parse a golden spectra document; every spectrum's content hash must match.
pub fn parse_golden_spectra(doc: &Node, file_sha256: &str) -> PResult<GoldenSpectra> {
    if doc.req("format")?.str()? != GOLDEN_SPECTRA_FORMAT {
        return Err(PluginError::schema("not a golden spectra file"));
    }
    if doc.req("format_version")?.v.as_u64() != Some(1) {
        return Err(PluginError::schema("unsupported format_version"));
    }
    let id = doc.req("id")?.str()?.to_string();
    let list = doc.req("spectra")?.arr()?;
    if list.is_empty() {
        return Err(doc.error("spectra: at least one spectrum required"));
    }
    let mut spectra = BTreeMap::new();
    for s in &list {
        let name = s.req("name")?.str()?.to_string();
        let x = s.req("values")?.vec_f64()?;
        if x.is_empty() {
            return Err(s.error(format!(
                "golden spectrum {name} must be a non-empty finite vector"
            )));
        }
        let start = s.req("wl_start_nm")?.f64()?;
        let step = s.req("wl_step_nm")?.f64()?;
        // numpy: wl_start + wl_step * arange(n)
        let wl: Vec<f64> = (0..x.len()).map(|i| start + step * i as f64).collect();
        let h = spectrum_sha256(&wl, &x);
        if h != s.req("sha256")?.str()? {
            return Err(PluginError::schema(format!(
                "golden spectrum {name} hash mismatch"
            )));
        }
        spectra.insert(h, GoldenSpectrum { name, wl, x });
    }
    Ok(GoldenSpectra {
        id,
        file_sha256: file_sha256.to_string(),
        spectra,
    })
}

/// Load a golden spectra file from disk.
pub fn load_golden_spectra(path: &Path) -> PResult<GoldenSpectra> {
    let (mut v, sha, _) = read_json_file(path)?;
    if let Some(base) = path.parent() {
        resolve_sidecars(&mut v, base)?;
    }
    parse_golden_spectra(&Node::root(&v), &sha)
}

/// Golden tolerance of a file.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Tolerance {
    pub abs: f64,
    pub rel: f64,
}

impl Tolerance {
    pub const MAX: Tolerance = Tolerance {
        abs: GOLDEN_MAX_TOL,
        rel: GOLDEN_MAX_TOL,
    };

    /// |got - want| / (abs + rel |want|): <= 1 passes.
    pub fn ratio(&self, got: f64, want: f64) -> f64 {
        (got - want).abs() / (self.abs + self.rel * want.abs() + 1e-300)
    }

    pub fn ok(&self, got: f64, want: f64) -> bool {
        got.is_finite() && (got - want).abs() <= self.abs + self.rel * want.abs()
    }
}

/// The tolerance a golden block is checked at: missing = the engine maximum; a looser one rejects the file.
pub fn tolerance(golden: &Node) -> PResult<Tolerance> {
    let Some(t) = golden.opt("tolerance") else {
        return Ok(Tolerance::MAX);
    };
    let mut out = Tolerance::MAX;
    for key in ["abs", "rel"] {
        let v = match t.opt(key) {
            Some(n) if n.v.is_number() => n.f64()?,
            _ => {
                return Err(PluginError::golden(format!(
                    "golden tolerance '{key}' must be a finite number >= 0"
                )))
            }
        };
        if v < 0.0 {
            return Err(PluginError::golden(format!(
                "golden tolerance '{key}' must be a finite number >= 0"
            )));
        }
        if v > GOLDEN_MAX_TOL {
            return Err(PluginError::golden(format!(
                "golden tolerance {key}={v:e} is looser than the engine maximum {GOLDEN_MAX_TOL:e}"
            )));
        }
        if key == "abs" {
            out.abs = v;
        } else {
            out.rel = v;
        }
    }
    Ok(out)
}

/// One failing comparison.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Failure {
    pub case: String,
    pub key: String,
    pub got: String,
    pub want: String,
}

/// The outcome of a golden run that could test something.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GoldenRun {
    /// Comparisons made (counted as spyder_ref counts them).
    pub checks: usize,
    pub failures: Vec<Failure>,
    /// Largest |got - want| / (abs + rel |want|) over the numeric comparisons (<= 1 passes).
    pub worst_ratio: f64,
    pub tolerance: Tolerance,
}

impl GoldenRun {
    /// A run with no comparisons yet.
    pub fn empty(tolerance: Tolerance) -> Self {
        Self::new(tolerance)
    }

    fn new(tolerance: Tolerance) -> Self {
        GoldenRun {
            checks: 0,
            failures: Vec::new(),
            worst_ratio: 0.0,
            tolerance,
        }
    }

    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    fn fail(&mut self, case: &str, key: &str, got: impl Into<String>, want: impl Into<String>) {
        self.checks += 1;
        self.failures.push(Failure {
            case: case.to_string(),
            key: key.to_string(),
            got: got.into(),
            want: want.into(),
        });
    }

    /// Scalar comparison (non-finite got fails).
    pub fn scalar(&mut self, case: &str, key: &str, got: Option<f64>, want: f64) {
        match got {
            None => self.fail(
                case,
                &format!("{key} (not produced)"),
                "none",
                want.to_string(),
            ),
            Some(g) if !g.is_finite() => self.fail(
                case,
                &format!("{key} (non-finite output)"),
                g.to_string(),
                want.to_string(),
            ),
            Some(g) => {
                let r = self.tolerance.ratio(g, want);
                self.worst_ratio = self.worst_ratio.max(r);
                if self.tolerance.ok(g, want) {
                    self.checks += 1;
                } else {
                    self.fail(case, key, format!("{g:e}"), format!("{want:e}"));
                }
            }
        }
    }

    /// Vector comparison counted as ONE check (spyder_ref: max err/tol <= 1).
    pub fn vector(&mut self, case: &str, key: &str, got: &[f64], want: &[f64]) {
        if got.len() != want.len() {
            return self.fail(
                case,
                &format!("{key} (dimension)"),
                got.len().to_string(),
                want.len().to_string(),
            );
        }
        if got.iter().any(|g| !g.is_finite()) {
            return self.fail(case, &format!("{key} (non-finite)"), "non-finite", "finite");
        }
        let r = got
            .iter()
            .zip(want)
            .map(|(g, w)| self.tolerance.ratio(*g, *w))
            .fold(0.0, f64::max);
        self.worst_ratio = self.worst_ratio.max(r);
        if r <= 1.0 {
            self.checks += 1;
        } else {
            self.fail(case, &format!("{key}(max err/tol)"), format!("{r:e}"), "1");
        }
    }
}

/// The expected value of one output: a number, a list of numbers, or a label.
enum Want {
    Num(Vec<f64>),
    Label(String),
}

fn want(case: &str, key: &str, n: &Node) -> PResult<Want> {
    match n.v {
        Value::String(s) => Ok(Want::Label(s.clone())),
        Value::Number(_) => Ok(Want::Num(vec![n.f64().map_err(|_| {
            PluginError::golden(format!("case {case}: expected '{key}' is not finite"))
        })?])),
        Value::Array(a) => {
            if a.is_empty() {
                return Err(PluginError::golden(format!(
                    "case {case}: expected '{key}' is empty"
                )));
            }
            let v = n.vec_f64().map_err(|_| {
                PluginError::golden(format!(
                    "case {case}: expected '{key}' must be a flat list of finite numbers"
                ))
            })?;
            Ok(Want::Num(v))
        }
        _ => Err(PluginError::golden(format!(
            "case {case}: expected '{key}' must be a number, a list of numbers or a label"
        ))),
    }
}

/// Cases of a golden block: at least `GOLDEN_MIN_CASES`.
pub fn cases<'a>(golden: &Node<'a>) -> PResult<Vec<Node<'a>>> {
    let n = match golden.opt("cases") {
        Some(c) => c
            .arr()
            .map_err(|_| PluginError::golden("golden: cases must be a list"))?,
        None => Vec::new(),
    };
    if n.len() < GOLDEN_MIN_CASES {
        return Err(PluginError::golden(format!(
            "golden: at least {GOLDEN_MIN_CASES} cases required, found {} (goldens must test something)",
            n.len()
        )));
    }
    Ok(n)
}

fn case_name(c: &Node, i: usize) -> String {
    c.opt("name")
        .and_then(|n| n.v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("case {i}"))
}

fn expected<'a>(c: &Node<'a>, name: &str) -> PResult<Vec<(String, Node<'a>)>> {
    let e = match c.opt("expected") {
        Some(e) if e.v.is_object() => e.entries()?,
        _ => Vec::new(),
    };
    if e.is_empty() {
        return Err(PluginError::golden(format!(
            "golden case {name}: empty 'expected' block"
        )));
    }
    Ok(e)
}

fn spectrum<'g>(c: &Node, name: &str, gs: &'g GoldenSpectra) -> PResult<&'g GoldenSpectrum> {
    let sha = c
        .opt("spectrum_sha256")
        .and_then(|s| s.v.as_str())
        .unwrap_or("");
    gs.spectra.get(sha).ok_or_else(|| {
        PluginError::golden(format!(
            "golden case {name}: spectrum {} not in the golden spectra file",
            super::short(sha, 12)
        ))
    })
}

/// Run a model's goldens (regression or consensus).
pub fn run_model_goldens(m: &Model, doc: &Node, gs: &GoldenSpectra) -> PResult<GoldenRun> {
    let golden = doc.req("golden")?;
    let tol = tolerance(&golden)?;
    let cases = cases(&golden)?;
    let p = m.features_nm.len();
    let (mut known, mut required): (Vec<String>, Vec<String>) = match m.kind {
        ModelKind::Regression => (
            ["value", "linear", "T2", "Q", "domain_ratio"]
                .map(String::from)
                .to_vec(),
            vec!["value".to_string()],
        ),
        ModelKind::Consensus => (vec!["value".into()], vec!["value".into()]),
    };
    if let crate::model::Body::Consensus { components } = &m.body {
        for c in components {
            known.push(format!("component:{}", c.name));
            required.push(format!("component:{}", c.name));
        }
    }
    let mut run = GoldenRun::new(tol);
    let mut any_features = false;
    for (ci, case) in cases.iter().enumerate() {
        let name = case_name(case, ci);
        let exp = expected(case, &name)?;
        let missing: Vec<&String> = required
            .iter()
            .filter(|k| !exp.iter().any(|(e, _)| e == *k))
            .collect();
        if !missing.is_empty() {
            return Err(PluginError::golden(format!(
                "golden case {name}: expected outputs missing for a {:?} file: {missing:?}",
                m.kind
            )));
        }
        let unknown: Vec<&String> = exp
            .iter()
            .map(|(k, _)| k)
            .filter(|k| !known.contains(k))
            .collect();
        if !unknown.is_empty() {
            return Err(PluginError::golden(format!(
                "golden case {name}: unknown expected outputs {unknown:?}"
            )));
        }
        let gsp = spectrum(case, &name, gs)?;
        let ctx = ScanContext::from_json(case.opt("scan_context").as_ref())?;
        let s = Spectrum::new(gsp.wl.clone(), gsp.x.clone());
        let n0 = run.checks;
        let pred = m.predict(&s, &ctx).map_err(|e| {
            PluginError::new(
                ErrorKind::GoldenFailed,
                format!("golden case {name}: the model could not run: {e}"),
            )
        })?;
        for (key, wn) in &exp {
            match want(&name, key, wn)? {
                Want::Label(l) => run.fail(
                    &name,
                    &format!("{key} (label where a number is produced)"),
                    "number",
                    l,
                ),
                Want::Num(w) if w.len() != 1 => run.fail(
                    &name,
                    &format!("{key} (dimension)"),
                    "1",
                    w.len().to_string(),
                ),
                Want::Num(w) => run.scalar(&name, key, pred.output(key), w[0]),
            }
        }
        if let Some(f) = case.opt("features") {
            if p == 0 {
                return Err(PluginError::golden(format!(
                    "golden case {name}: 'features' given but the file has no feature list"
                )));
            }
            let w = match want(&name, "features", &f)? {
                Want::Num(w) => w,
                Want::Label(_) => {
                    return Err(PluginError::golden(format!(
                        "golden case {name}: features must be numbers"
                    )))
                }
            };
            if w.len() != p {
                return Err(PluginError::golden(format!(
                    "golden case {name}: {} features, the model declares {p}",
                    w.len()
                )));
            }
            any_features = true;
            match m.features(&s, &ctx) {
                Ok(got) => run.vector(&name, "features", &got, &w),
                Err(e) => run.fail(&name, "features (chain failed)", e.to_string(), "features"),
            }
        }
        if run.checks == n0 {
            return Err(PluginError::golden(format!(
                "golden case {name}: zero comparisons"
            )));
        }
    }
    if run.checks == 0 {
        return Err(PluginError::golden("golden: zero comparisons"));
    }
    if m.kind == ModelKind::Regression && p > 0 && !any_features {
        return Err(PluginError::golden(
            "golden: no case carries the intermediate feature vector",
        ));
    }
    Ok(run)
}

/// Grid label of an output channel: str(int(round(wl))) as spyder_ref writes `transferred_at` keys.
pub fn wl_key(wl: f64) -> String {
    format!("{}", wl.round_ties_even() as i64)
}

/// Run a transfer file's goldens (`expected.transferred_at`: {"<nm>": value}).
pub fn run_transfer_goldens(t: &Transfer, doc: &Node, gs: &GoldenSpectra) -> PResult<GoldenRun> {
    let golden = doc.req("golden")?;
    let tol = tolerance(&golden)?;
    let cases = cases(&golden)?;
    let mut run = GoldenRun::new(tol);
    for (ci, case) in cases.iter().enumerate() {
        let name = case_name(case, ci);
        let exp = expected(case, &name)?;
        if !exp.iter().any(|(k, _)| k == "transferred_at") {
            return Err(PluginError::golden(format!(
                "golden case {name}: expected outputs missing for a transfer: [\"transferred_at\"]"
            )));
        }
        if let Some((k, _)) = exp.iter().find(|(k, _)| k != "transferred_at") {
            return Err(PluginError::golden(format!(
                "golden case {name}: unknown expected outputs [{k:?}]"
            )));
        }
        let gsp = spectrum(case, &name, gs)?;
        let ctx = ScanContext::from_json(case.opt("scan_context").as_ref())?;
        let n0 = run.checks;
        let out = t
            .apply(&Spectrum::new(gsp.wl.clone(), gsp.x.clone()), &ctx)
            .map_err(|e| {
                PluginError::new(
                    ErrorKind::GoldenFailed,
                    format!("golden case {name}: the transfer could not run: {e}"),
                )
            })?;
        if out.x.iter().any(|v| !v.is_finite()) {
            run.fail(
                &name,
                "transferred (non-finite output)",
                "non-finite",
                "finite",
            );
        }
        let got: BTreeMap<String, f64> = out
            .wl
            .iter()
            .zip(&out.x)
            .map(|(w, v)| (wl_key(*w), *v))
            .collect();
        let (_, ta) = &exp[0];
        let ta = exp
            .iter()
            .find(|(k, _)| k == "transferred_at")
            .map(|(_, n)| n)
            .unwrap_or(ta);
        let entries = match ta.v {
            Value::Object(_) => ta.entries()?,
            _ => Vec::new(),
        };
        if entries.is_empty() {
            return Err(PluginError::golden(format!(
                "golden case {name}: empty 'transferred_at'"
            )));
        }
        for (lam, wv) in &entries {
            let w = wv.f64().map_err(|_| {
                PluginError::golden(format!(
                    "golden case {name}: transferred_at[{lam}] must be a finite number"
                ))
            })?;
            let key = format!("T@{lam}");
            match got.get(lam) {
                None => run.fail(
                    &name,
                    &format!("{key} (not on the output grid)"),
                    "none",
                    w.to_string(),
                ),
                Some(g) => run.scalar(&name, &key, Some(*g), w),
            }
        }
        if run.checks == n0 {
            return Err(PluginError::golden(format!(
                "golden case {name}: zero comparisons"
            )));
        }
    }
    if run.checks == 0 {
        return Err(PluginError::golden("golden: zero comparisons"));
    }
    Ok(run)
}

/// Structural check of a golden block whose evaluation belongs to a later engine phase (engine checks and
/// analysis profiles, Phase 3): tolerance within the engine maximum, at least 3 cases, every case named with
/// a non-empty `expected` block. Returns the number of cases.
pub fn check_deferred_block(golden: &Node) -> PResult<usize> {
    tolerance(golden)?;
    let cs = cases(golden)?;
    for (i, c) in cs.iter().enumerate() {
        let name = case_name(c, i);
        expected(c, &name)?;
    }
    Ok(cs.len())
}
