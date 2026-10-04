//! Model files (`spyder-bone/model`, PLAN section 4; 04 section 2): linear regression
//! y = offset + sum_i b_i (x_i - c_i) over the model's own preprocessing chain and explicit feature list, and the
//! `consensus` kind (CONS3: the median of an odd number of components, each folded exactly into one linear
//! functional of the processed spectrum). A port of spyder_ref `check_model`, `_check_consensus`,
//! `_check_snv_chain`, `predict` and `_domain`.
//!
//! The classifier and rule kinds exist in the format but are reserved in the v1 engine: such a file is disabled
//! with "unsupported model kind", never guessed.

// `!(x > 0.0)` is deliberate: it rejects NaN as well as non-positive values.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use serde::Serialize;

use crate::grid::{self, Consumer, WL_TOL};
use crate::plugins::{
    is_sha256_hex, parse_header, ErrorKind, Header, Node, PResult, PluginError, ScanContext,
    Version, MODEL_FORMAT,
};
use crate::preprocess::{self, Op, OpError, SgMode, Spectrum};
use crate::status::Status;

/// Model kinds the v1 engine evaluates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Regression,
    Consensus,
}

/// Inverse output transform applied after the linear part.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub enum YTransform {
    None,
    Log10 { offset: f64 },
    Ln { offset: f64 },
    Sqrt { offset: f64 },
}

/// One soft domain level: a value gets the first label whose `below` it is under; the last has none.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Level {
    pub label: String,
    pub below: Option<f64>,
}

/// PLS T^2 / Q domain statistics (inform only; never blocks).
#[derive(Debug, Clone, PartialEq)]
pub struct Domain {
    pub x_center: Vec<f64>,
    /// Per component: weights r (length p).
    pub weights_r: Vec<Vec<f64>>,
    /// Per component: loadings p (length p).
    pub loadings_p: Vec<Vec<f64>>,
    pub score_var: Vec<f64>,
    pub t2_limit: f64,
    pub q_limit: f64,
    pub levels: Vec<Level>,
}

/// One CONS3 component: identity of the folded model and its linear functional.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Component {
    pub name: String,
    pub label: Option<String>,
    pub id: String,
    pub version: Version,
    pub sha256: String,
    pub wavelengths_nm: Vec<f64>,
    pub weights: Vec<f64>,
    pub offset: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Regression {
        coefficients: Vec<f64>,
        x_center: Option<Vec<f64>>,
        offset: f64,
    },
    Consensus {
        components: Vec<Component>,
    },
}

/// A parsed, structurally checked model file.
#[derive(Debug, Clone)]
pub struct Model {
    pub header: Header,
    pub kind: ModelKind,
    pub role: String,
    pub short_name: Option<String>,
    pub trained_on_class: String,
    pub also_valid_for: Vec<String>,
    pub training_splices_nm: Vec<f64>,
    pub chain: Vec<Op>,
    pub features_nm: Vec<f64>,
    pub body: Body,
    pub y_transform: YTransform,
    pub clip: (Option<f64>, Option<f64>),
    pub domain: Option<Domain>,
    /// RMSECV for the gentle "+/- about" hint, when the file gives one.
    pub error_rmsecv: Option<f64>,
    pub decimals: Option<u64>,
}

/// A model's outputs on one spectrum.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Prediction {
    /// The reading (after the inverse transform and clip; the median for a consensus).
    pub value: f64,
    /// The linear part (regression only).
    pub linear: Option<f64>,
    #[serde(rename = "T2")]
    pub t2: Option<f64>,
    #[serde(rename = "Q")]
    pub q: Option<f64>,
    pub domain_ratio: Option<f64>,
    pub domain_level: Option<String>,
    /// `component:<name>` values of a consensus, in file order.
    pub components: Vec<(String, f64)>,
}

impl Prediction {
    /// Named numeric outputs as the golden runner sees them (`value`, `linear`, `T2`, `Q`, `domain_ratio`,
    /// `component:<name>`).
    pub fn output(&self, key: &str) -> Option<f64> {
        match key {
            "value" => Some(self.value),
            "linear" => self.linear,
            "T2" => self.t2,
            "Q" => self.q,
            "domain_ratio" => self.domain_ratio,
            k => {
                let name = k.strip_prefix("component:")?;
                self.components
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, v)| *v)
            }
        }
    }
}

const ROLES: [&str; 5] = [
    "collagen_percent",
    "collagen_class",
    "organics_index",
    "contamination_warning",
    "other",
];

fn id_pattern_ok(id: &str) -> bool {
    // ^[a-z0-9]+(\.[a-z0-9_]+)+$
    let mut parts = id.split('.');
    let first = parts.next().unwrap_or("");
    let rest: Vec<&str> = parts.collect();
    !first.is_empty()
        && first
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        && !rest.is_empty()
        && rest.iter().all(|p| {
            !p.is_empty()
                && p.bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        })
}

fn levels(n: &Node) -> PResult<Vec<Level>> {
    let a = n.arr()?;
    if a.is_empty() {
        return Err(n.error("needs at least one level"));
    }
    let mut out = Vec::new();
    for (i, l) in a.iter().enumerate() {
        let below = l.opt("below").map(|b| b.f64()).transpose()?;
        if i + 1 < a.len() && below.is_none() {
            return Err(l.error("every level but the last needs \"below\""));
        }
        out.push(Level {
            label: l.req("label")?.str()?.to_string(),
            below,
        });
    }
    Ok(out)
}

/// The first label whose `below` the value is under, else the last label (spyder_ref `_levels`).
pub fn level_for(v: f64, levels: &[Level]) -> Option<String> {
    let last = levels.last()?;
    for l in &levels[..levels.len() - 1] {
        if let Some(b) = l.below {
            if v < b {
                return Some(l.label.clone());
            }
        }
    }
    Some(last.label.clone())
}

/// Parse and check a model document (sidecars already resolved).
pub fn parse_model(doc: &Node) -> PResult<Model> {
    let header = parse_header(doc, MODEL_FORMAT)?;
    if !id_pattern_ok(&header.id) {
        return Err(PluginError::schema(format!(
            "id {:?} must be dotted lower-case (e.g. collagen.ryder2026.2045)",
            header.id
        )));
    }
    for k in ["title", "created", "exported"] {
        doc.req(k)?.str()?;
    }
    doc.req("citation")?.req("text")?.str()?;
    doc.req("provenance")?.obj()?;
    doc.req("golden")?.obj()?;
    let kind = match doc.req("kind")?.str()? {
        "regression" => ModelKind::Regression,
        "consensus" => ModelKind::Consensus,
        k @ ("classifier" | "rule") => {
            return Err(PluginError::new(
                ErrorKind::Unsupported,
                format!("unsupported model kind {k:?} (reserved; the v1 engine runs regression and consensus files)"),
            ))
        }
        k => return Err(doc.req("kind")?.error(format!("unknown model kind {k:?}"))),
    };
    let role = doc.req("role")?.one_of(&ROLES)?.to_string();
    let short_name = doc
        .opt("short_name")
        .map(|s| s.str().map(str::to_string))
        .transpose()?;

    let inst = doc.req("instrument")?;
    let trained_on_class = inst.req("trained_on_class")?.str()?.to_string();
    let also_valid_for = inst
        .opt("also_valid_for")
        .map(|a| a.vec_str())
        .transpose()?
        .unwrap_or_default();
    let training_splices_nm = inst
        .opt("training_splices_nm")
        .map(|a| a.vec_f64_len(2))
        .transpose()?
        .unwrap_or_default();
    if let Some(g) = inst.opt("grid") {
        grid_spec(&g)?;
    }
    let input = doc.req("input")?;
    input.obj()?;
    if let Some(q) = input.opt("quantity") {
        if q.one_of(&["reflectance", "absorbance"])? != "reflectance" {
            return Err(PluginError::new(
                ErrorKind::Unsupported,
                "input quantity \"absorbance\" is not in the v1 engine (models read reflectance)",
            ));
        }
    }
    if let Some(s) = input.opt("scale") {
        if s.one_of(&["fraction", "percent"])? != "fraction" {
            return Err(PluginError::new(
                ErrorKind::Unsupported,
                "input scale \"percent\" is not in the v1 engine (reflectance as a fraction)",
            ));
        }
    }

    let pre = doc.req("preprocessing")?;
    pre.arr()?;
    let chain = preprocess::parse_chain(pre.v)?;
    let feats = doc.req("features")?;
    let features_nm = feats.req("wavelengths_nm")?.vec_f64()?;
    let p = features_nm.len();

    let body = match kind {
        ModelKind::Regression => {
            if p == 0 {
                return Err(PluginError::schema("regression with no features"));
            }
            let r = doc.req("regression")?;
            let coefficients = r
                .req("coefficients")?
                .vec_f64_len(p)
                .map_err(|e| PluginError::schema(format!("coefficient length mismatch: {e}")))?;
            let x_center = r.opt("x_center").map(|x| x.vec_f64_len(p)).transpose()?;
            let offset = r.req("offset")?.f64()?;
            Body::Regression {
                coefficients,
                x_center,
                offset,
            }
        }
        ModelKind::Consensus => {
            if p != 0 {
                return Err(PluginError::schema(
                    "a consensus file declares no top-level features (each component carries its own)",
                ));
            }
            if header.engine_min < (1, 1) {
                return Err(PluginError::schema("kind consensus needs engine_min 1.1"));
            }
            Body::Consensus {
                components: consensus_components(&doc.req("consensus")?)?,
            }
        }
    };

    let (y_transform, clip, decimals) = match doc.opt("output") {
        None if kind == ModelKind::Regression => {
            return Err(PluginError::schema("missing required field \"output\""))
        }
        None => (YTransform::None, (None, None), None),
        Some(o) => {
            let yt = match o.opt("y_transform") {
                None => YTransform::None,
                Some(t) => {
                    let off = t.opt("offset").map(|x| x.f64()).transpose()?.unwrap_or(0.0);
                    match t
                        .opt("type")
                        .map(|x| x.str())
                        .transpose()?
                        .unwrap_or("none")
                    {
                        "none" => YTransform::None,
                        "log10" => YTransform::Log10 { offset: off },
                        "ln" => YTransform::Ln { offset: off },
                        "sqrt" => YTransform::Sqrt { offset: off },
                        other => return Err(t.error(format!("unknown y_transform {other:?}"))),
                    }
                }
            };
            let clip = match o.opt("clip") {
                None => (None, None),
                Some(c) => (
                    c.opt("min").map(|x| x.f64()).transpose()?,
                    c.opt("max").map(|x| x.f64()).transpose()?,
                ),
            };
            let dec = o.opt("decimals").map(|d| d.u64()).transpose()?;
            (yt, clip, dec)
        }
    };
    if kind == ModelKind::Consensus && y_transform != YTransform::None {
        return Err(PluginError::schema(
            "a consensus has no y_transform (the median of the components is the value)",
        ));
    }

    let domain = match doc.opt("domain") {
        None => None,
        Some(d) => match d.req("method")?.one_of(&["none", "pls_t2_q"])? {
            "none" => None,
            _ => {
                if kind != ModelKind::Regression {
                    return Err(d.error("pls_t2_q needs a regression model"));
                }
                Some(parse_domain(&d, p, &body)?)
            }
        },
    };
    let error_rmsecv = doc
        .opt("error")
        .and_then(|e| e.opt("rmsecv"))
        .map(|x| x.f64())
        .transpose()?;

    let m = Model {
        header,
        kind,
        role,
        short_name,
        trained_on_class,
        also_valid_for,
        training_splices_nm,
        chain,
        features_nm,
        body,
        y_transform,
        clip,
        domain,
        error_rmsecv,
        decimals,
    };
    check_snv_chain(&m, doc)?;
    Ok(m)
}

fn grid_spec(g: &Node) -> PResult<(f64, f64, f64)> {
    let start = g.req("start_nm")?.f64()?;
    let step = g.req("step_nm")?.f64()?;
    let n = g.req("n")?.u64()?;
    if !(step > 0.0) || n < 2 {
        return Err(g.error("grid needs step_nm > 0 and n >= 2"));
    }
    Ok((start, start + step * (n as f64 - 1.0), step))
}

fn parse_domain(d: &Node, p: usize, body: &Body) -> PResult<Domain> {
    let x_center = match d.opt("x_center") {
        Some(x) => x.vec_f64_len(p)?,
        None => match body {
            Body::Regression {
                x_center: Some(c), ..
            } => c.clone(),
            _ => return Err(d.error("needs x_center (or the regression's x_center)")),
        },
    };
    let comps = d.req("components")?.arr()?;
    if comps.is_empty() {
        return Err(d.error("needs at least one component"));
    }
    let (mut wr, mut lp, mut sv) = (Vec::new(), Vec::new(), Vec::new());
    for c in &comps {
        wr.push(c.req("weights_r")?.vec_f64_len(p)?);
        lp.push(c.req("loadings_p")?.vec_f64_len(p)?);
        let s = c.req("score_var")?.f64()?;
        if !(s > 0.0) {
            return Err(c.error("score_var must be > 0"));
        }
        sv.push(s);
    }
    let t2_limit = d.req("t2_limit")?.f64()?;
    let q_limit = d.req("q_limit")?.f64()?;
    if !(t2_limit > 0.0 && q_limit > 0.0) {
        return Err(d.error("t2_limit and q_limit must be > 0"));
    }
    Ok(Domain {
        x_center,
        weights_r: wr,
        loadings_p: lp,
        score_var: sv,
        t2_limit,
        q_limit,
        levels: levels(&d.req("levels")?)?,
    })
}

fn consensus_components(c: &Node) -> PResult<Vec<Component>> {
    let allowed = ["combine", "components"];
    for (k, _) in c.entries()? {
        if !allowed.contains(&k.as_str()) {
            return Err(c.error(format!("unexpected key {k:?}")));
        }
    }
    if c.req("combine")?.str()? != "median" {
        return Err(c.error("consensus.combine must be \"median\""));
    }
    let comps = c.req("components")?.arr()?;
    if comps.len() < 3 || comps.len() % 2 != 1 {
        return Err(c.error("consensus needs an odd number (>= 3) of components"));
    }
    let mut out: Vec<Component> = Vec::new();
    for k in &comps {
        let name = k.req("name")?.str()?.to_string();
        if name.is_empty() || out.iter().any(|o| o.name == name) {
            return Err(k.error(format!("component name {name:?} empty or repeated")));
        }
        let vs = k.req("version")?.str()?;
        let version = Version::parse(vs)
            .ok_or_else(|| k.error(format!("version {vs:?} must be MAJOR.MINOR.PATCH")))?;
        let sha256 = k.req("sha256")?.str()?.to_string();
        if !is_sha256_hex(&sha256) {
            return Err(k.error("sha256 must be 64 lower-case hex characters"));
        }
        let f = k.req("feature")?;
        if f.req("type")?.str()? != "linear" {
            return Err(f.error("feature type must be \"linear\""));
        }
        let wl = f.req("wavelengths_nm")?.vec_f64()?;
        if wl.is_empty() {
            return Err(f.error("no wavelengths"));
        }
        let weights = f.req("weights")?.vec_f64_len(wl.len())?;
        let offset = f.opt("offset").map(|o| o.f64()).transpose()?.unwrap_or(0.0);
        out.push(Component {
            name,
            label: k
                .opt("label")
                .map(|l| l.str().map(str::to_string))
                .transpose()?,
            id: k.req("id")?.str()?.to_string(),
            version,
            sha256,
            wavelengths_nm: wl,
            weights,
            offset,
        });
    }
    Ok(out)
}

/// spyder_ref `_check_snv_chain`: block SNV is the last step, its blocks lie inside the grid, at least an SG
/// half-window from the edges of the grid the last SG ran on, never across a training join, and every feature
/// lies inside a block.
fn check_snv_chain(m: &Model, doc: &Node) -> PResult<()> {
    let (mut lo, mut hi, step) = match doc.req("instrument")?.opt("grid") {
        Some(g) => grid_spec(&g)?,
        None => match doc.req("input")?.opt("range_nm") {
            Some(r) => {
                let (a, b) = r.range(false)?;
                (a, b, 1.0)
            }
            None => (350.0, 2500.0, 1.0),
        },
    };
    let mut last_sg: Option<(f64, f64, f64)> = None;
    let n_ops = m.chain.len();
    for (i, op) in m.chain.iter().enumerate() {
        match op {
            Op::Crop { lo: a, hi: b } => {
                lo = lo.max(*a);
                hi = hi.min(*b);
            }
            Op::SavGol(p) => {
                let h = (p.window / 2) as f64;
                last_sg = Some((lo, hi, h * step));
                if p.mode == SgMode::Valid {
                    lo += h * step;
                    hi -= h * step;
                }
            }
            Op::BlockSnv { blocks_nm, .. } => {
                if m.header.engine_min < (1, 1) {
                    return Err(PluginError::schema(
                        "snv blocks_nm needs engine_min 1.1 (an older engine would ignore the blocks)",
                    ));
                }
                if i != n_ops - 1 {
                    return Err(PluginError::schema(
                        "block SNV must be the last preprocessing step",
                    ));
                }
                for &(b0, b1) in blocks_nm {
                    if b0 < lo - WL_TOL || b1 > hi + WL_TOL {
                        return Err(PluginError::schema(format!(
                            "snv block {b0}-{b1} nm lies outside the grid {lo}-{hi} nm"
                        )));
                    }
                    if let Some((g0, g1, hh)) = last_sg {
                        if b0 - hh < g0 - WL_TOL || b1 + hh > g1 + WL_TOL {
                            return Err(PluginError::schema(format!(
                                "snv block {b0}-{b1} nm is within the SG half-window ({hh} nm) of the grid edge \
                                 {g0}-{g1} nm the SG ran on: SG edge values would enter the block"
                            )));
                        }
                    }
                    for &j in &m.training_splices_nm {
                        if b0 <= j + WL_TOL && b1 > j + WL_TOL {
                            return Err(PluginError::schema(format!(
                                "snv block {b0}-{b1} nm spans the detector join at {j} nm"
                            )));
                        }
                    }
                }
                for &lam in &m.features_nm {
                    if !blocks_nm
                        .iter()
                        .any(|&(b0, b1)| b0 - WL_TOL <= lam && lam <= b1 + WL_TOL)
                    {
                        return Err(PluginError::schema(format!(
                            "feature {lam} nm lies outside every SNV block"
                        )));
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// Plain left-to-right dot product.
fn dot(a: &[f64], b: &[f64]) -> f64 {
    let mut s = 0.0;
    for (x, y) in a.iter().zip(b) {
        s += x * y;
    }
    s
}

/// numpy `np.maximum` / `np.minimum` with NaN propagating.
fn np_max(x: f64, c: f64) -> f64 {
    if x.is_nan() || x >= c {
        x
    } else {
        c
    }
}
fn np_min(x: f64, c: f64) -> f64 {
    if x.is_nan() || x <= c {
        x
    } else {
        c
    }
}

impl Model {
    /// `id@version`.
    pub fn key(&self) -> String {
        self.header.key()
    }

    /// The processed spectrum (the model's chain on the given stream).
    pub fn processed(&self, s: &Spectrum, ctx: &ScanContext) -> Result<Spectrum, OpError> {
        preprocess::run_chain(&self.chain, s, &ctx.splices_nm)
    }

    /// The feature vector (regression), i.e. the processed spectrum at `features.wavelengths_nm`.
    pub fn features(&self, s: &Spectrum, ctx: &ScanContext) -> Result<Vec<f64>, OpError> {
        let z = self.processed(s, ctx)?;
        preprocess::select_wavelengths(&z, &self.features_nm)
    }

    fn inverse_y(&self, v: f64) -> f64 {
        match self.y_transform {
            YTransform::None => v,
            YTransform::Log10 { offset } => 10f64.powf(v) - offset,
            YTransform::Ln { offset } => v.exp() - offset,
            YTransform::Sqrt { offset } => v * v - offset,
        }
    }

    /// Predict on one spectrum (already on the model's stream: transferred if a transfer applies).
    pub fn predict(&self, s: &Spectrum, ctx: &ScanContext) -> Result<Prediction, OpError> {
        let z = self.processed(s, ctx)?;
        match &self.body {
            Body::Consensus { components } => {
                let mut comps = Vec::with_capacity(components.len());
                for c in components {
                    let x = preprocess::select_wavelengths(&z, &c.wavelengths_nm)?;
                    comps.push((c.name.clone(), dot(&x, &c.weights) + c.offset));
                }
                let vals: Vec<f64> = comps.iter().map(|(_, v)| *v).collect();
                // np.median propagates NaN
                let value = if vals.iter().any(|v| v.is_nan()) {
                    f64::NAN
                } else {
                    crate::n2::median(&vals)
                };
                Ok(Prediction {
                    value,
                    linear: None,
                    t2: None,
                    q: None,
                    domain_ratio: None,
                    domain_level: None,
                    components: comps,
                })
            }
            Body::Regression {
                coefficients,
                x_center,
                offset,
            } => {
                let f = preprocess::select_wavelengths(&z, &self.features_nm)?;
                let centred: Vec<f64> = match x_center {
                    Some(c) => f.iter().zip(c).map(|(a, b)| a - b).collect(),
                    None => f.clone(),
                };
                let lin = offset + dot(&centred, coefficients);
                let mut val = self.inverse_y(lin);
                if let Some(mn) = self.clip.0 {
                    val = np_max(val, mn);
                }
                if let Some(mx) = self.clip.1 {
                    val = np_min(val, mx);
                }
                let mut p = Prediction {
                    value: val,
                    linear: Some(lin),
                    t2: None,
                    q: None,
                    domain_ratio: None,
                    domain_level: None,
                    components: Vec::new(),
                };
                if let Some(d) = &self.domain {
                    let e: Vec<f64> = f.iter().zip(&d.x_center).map(|(a, b)| a - b).collect();
                    let t: Vec<f64> = d.weights_r.iter().map(|r| dot(&e, r)).collect();
                    let mut t2 = 0.0;
                    for (ta, sv) in t.iter().zip(&d.score_var) {
                        t2 += ta * ta / sv;
                    }
                    let mut q = 0.0;
                    for (i, ei) in e.iter().enumerate() {
                        let mut recon = 0.0;
                        for (ta, pa) in t.iter().zip(&d.loadings_p) {
                            recon += ta * pa[i];
                        }
                        let r = ei - recon;
                        q += r * r;
                    }
                    let ratio = np_max(t2 / d.t2_limit, q / d.q_limit);
                    p.t2 = Some(t2);
                    p.q = Some(q);
                    p.domain_ratio = Some(ratio);
                    p.domain_level = level_for(ratio, &d.levels);
                }
                Ok(p)
            }
        }
    }

    /// Wavelength regions the model reads, each with the SG half-width (channels) its kernel adds: block SNV
    /// reads whole blocks; otherwise contiguous runs of features. Consensus components are folded (support
    /// already inside their wavelengths), so their half-width is 0.
    pub fn read_regions(&self, step: f64) -> Vec<((f64, f64), usize)> {
        let h = self
            .chain
            .iter()
            .filter_map(|op| match op {
                Op::SavGol(p) => Some(p.window / 2),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        if let Some(Op::BlockSnv { blocks_nm, .. }) = self
            .chain
            .iter()
            .find(|op| matches!(op, Op::BlockSnv { .. }))
        {
            return blocks_nm.iter().map(|&b| (b, h)).collect();
        }
        match &self.body {
            Body::Regression { .. } => runs(&self.features_nm, step)
                .into_iter()
                .map(|r| (r, h))
                .collect(),
            Body::Consensus { components } => components
                .iter()
                .flat_map(|c| runs(&c.wavelengths_nm, step))
                .map(|r| (r, h))
                .collect(),
        }
    }

    /// Grid eligibility on a scan (PLAN Step 1): every region +/- its SG half-width supplied by the grid and
    /// inside one detector segment of the scan's joins (models are never trained across a join).
    pub fn eligibility(&self, wl: &[f64], joins: &[f64]) -> Status {
        let step = grid::uniform_step(wl).unwrap_or(1.0);
        for (window_nm, h) in self.read_regions(step) {
            let c = Consumer {
                window_nm,
                sg_half_width: h,
                edge_margin: 0,
                trained_across_joins_nm: Vec::new(),
            };
            let s = grid::eligibility(wl, joins, &c);
            if !s.is_assessed() {
                return s;
            }
        }
        Status::Assessed
    }
}

/// Contiguous runs [lo, hi] of a wavelength list (a gap larger than 1.5 steps starts a new run).
fn runs(wl: &[f64], step: f64) -> Vec<(f64, f64)> {
    let mut v: Vec<f64> = wl.to_vec();
    v.sort_by(f64::total_cmp);
    let mut out: Vec<(f64, f64)> = Vec::new();
    for w in v {
        match out.last_mut() {
            Some(last) if w - last.1 <= 1.5 * step => last.1 = w,
            _ => out.push((w, w)),
        }
    }
    out
}
