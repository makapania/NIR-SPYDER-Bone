//! Shared-table plug-ins (PLAN section 4; spyder_ref `tables`): `bands.json`, `noise_gains.json` and the
//! instrument registry.
//!
//! * bands (format_version 2): band reading r_b = mean over centre +/- half_width of E = scale x SG derivative of
//!   log10(1/max(R, clip)) (kernel per band); readability SD_E = gain x envelope factor x N2(noise window) on the
//!   AS-MEASURED scan, the gain COMPUTED ([`super::gains`]) from the band kernel, the correction applied to the
//!   stream and the scan's own detector segments.
//! * noise gains (format_version 2): the noise model, the N2 windows a model kernel's detector parts use, per-model
//!   settings (B6 policy; a recorded gain for a non-linear model) and the consensus factor. Each model's implied SD
//!   (wt%) = sqrt(sum (gain x N2(window))^2), the gains computed the same way; a consensus = factor x median of its
//!   component SDs.
//! * Nothing is keyed by a transfer file's SHA-256 (format 1 listed gains per transfer key). An output whose gain
//!   cannot be computed is "not assessed" with the reason; a band's unknown SD is +infinity (unreadable unless
//!   strong: fail safe), never a guess.
//! * instruments: the classes and serial lists that PRESET the user's Standard / High-res switch (DECISIONS 50).

use std::collections::BTreeMap;

use serde::Serialize;

use super::gains::{self, parse_instrument_noise, Correction, GainCache, InstrumentNoise};
use super::golden::{cases, tolerance, GoldenRun, GoldenSpectra, Tolerance};
use super::{
    parse_header, parse_header_v, Header, Node, PResult, PluginError, BANDS_FORMAT,
    INSTRUMENT_FORMAT, NOISE_GAINS_FORMAT, TABLE_FORMAT_VERSION,
};
use crate::model::{Model, ModelKind};
use crate::n2::n2;
use crate::numsum::np_mean;
use crate::preprocess::{self, Op, SgMode, SgParams, Spectrum};

/// The key of a stream with no correction (the scan as measured).
pub const NO_TRANSFER: &str = "none";
/// Status of an output whose N2 is not finite.
pub const NOISE_UNKNOWN: &str = "not assessed: noise unknown (N2 not finite)";
/// B6 status of a reader the noise table exempts from B6 (`"b6": false`): its implied SD is still reported.
pub const B6_NOT_APPLIED: &str = "not assessed: B6 not applied to this reader";

/// A band kernel: E = scale x SG(log10(1/max(R, clip))).
#[derive(Debug, Clone, PartialEq)]
pub struct Kernel {
    pub absorbance_clip_min: f64,
    pub savgol: SgParams,
    /// True when the file states the SG edge mode (otherwise spyder_ref's default, interp, applies).
    pub mode_stated: bool,
    pub scale: f64,
}

/// Parse a kernel's `savgol` block; a missing `mode` defaults to "interp" as in spyder_ref `op_savgol`.
pub fn parse_sg(n: &Node) -> PResult<(SgParams, bool)> {
    let mut m = n.obj()?.clone();
    let stated = m.contains_key("mode");
    m.entry("mode").or_insert_with(|| "interp".into());
    m.entry("delta").or_insert_with(|| 1.0.into());
    m.insert("op".into(), "savgol".into());
    match preprocess::parse_op(&serde_json::Value::Object(m)) {
        Ok(preprocess::Op::SavGol(p)) => Ok((p, stated)),
        Ok(_) => Err(n.error("not a Savitzky-Golay kernel")),
        Err(e) => Err(n.error(e.to_string())),
    }
}

impl Kernel {
    /// E on a stream.
    pub fn apply(&self, s: &Spectrum) -> Result<Spectrum, preprocess::OpError> {
        let a = Spectrum::new(
            s.wl.clone(),
            preprocess::absorbance(&s.x, self.absorbance_clip_min),
        );
        let mut d = preprocess::savgol(&a, &self.savgol)?;
        for v in d.x.iter_mut() {
            *v *= self.scale;
        }
        Ok(d)
    }
}

/// Readability of one band: SD_E = gain x envelope factor x N2(noise window); the gain is computed.
#[derive(Debug, Clone, PartialEq)]
pub struct Readability {
    pub noise_window_nm: (f64, f64),
    /// The within-detector noise envelope at the band (the C-H bands near the detector ends; 1.0 elsewhere). A
    /// correction may carry its instrument's own factor (`provenance.instrument_noise.ch_envelope_factors`).
    pub envelope_factor: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Band {
    pub centre_nm: f64,
    pub half_width_nm: u64,
    pub kernel: String,
    pub detector: String,
    pub roles: Vec<String>,
    pub weight: Option<f64>,
    pub faint_u: Option<f64>,
    pub noise_window_nm: Option<(f64, f64)>,
    pub readability: Option<Readability>,
    /// The projection (a `projections` key) the band is read on, if any (the OH-corrected 1545 nm trough).
    pub projection: Option<String>,
    /// u = u_sign x r / weight (-1 for a trough; default +1).
    pub u_sign: f64,
}

/// A linear projection over a window of a kernel's E (phase 0c): x = E / scale, x' = x - v (v . (x - c)),
/// E' = scale x'. `direction` (unit length) and `centre` hold one value per grid point of the window.
#[derive(Debug, Clone, PartialEq)]
pub struct Projection {
    pub window_nm: (f64, f64),
    pub kernel: String,
    pub direction: Vec<f64>,
    pub centre: Vec<f64>,
}

impl Projection {
    /// E with the projection applied over its window (NaN elsewhere). The dot product is summed in channel order,
    /// as the oracle does (`kernels.projected`).
    pub fn apply(&self, e: &[f64], wl: &[f64], scale: f64) -> Vec<f64> {
        let mut out = vec![f64::NAN; e.len()];
        let (Some(a), Some(b)) = (
            crate::grid::index_of(wl, self.window_nm.0),
            crate::grid::index_of(wl, self.window_nm.1),
        ) else {
            return out;
        };
        if b < a || b - a + 1 != self.direction.len() || self.centre.len() != self.direction.len() {
            return out;
        }
        let x: Vec<f64> = e[a..=b].iter().map(|v| v / scale).collect();
        let mut d = 0.0;
        for ((xi, ci), vi) in x.iter().zip(&self.centre).zip(&self.direction) {
            d += (xi - ci) * vi;
        }
        for (o, (xi, vi)) in out[a..=b].iter_mut().zip(x.iter().zip(&self.direction)) {
            *o = scale * (xi - vi * d);
        }
        out
    }
}

/// `bands.json`.
#[derive(Debug, Clone)]
pub struct Bands {
    pub header: Header,
    pub kernels: BTreeMap<String, Kernel>,
    pub bands: BTreeMap<String, Band>,
    pub projections: BTreeMap<String, Projection>,
    /// The noise model's correlation length (nm) for the readability gains (`noise_model.correlation_nm`).
    pub correlation_nm: f64,
}

/// `noise_model.correlation_nm` of a table (finite, >= 0).
fn correlation_nm(doc: &Node) -> PResult<f64> {
    let nm = doc.req("noise_model")?;
    let c = nm.req("correlation_nm")?;
    let v = c.f64()?;
    if !(v >= 0.0) {
        return Err(c.error("correlation_nm must be a finite number >= 0"));
    }
    Ok(v)
}

pub fn parse_bands(doc: &Node) -> PResult<Bands> {
    let header = parse_header_v(doc, BANDS_FORMAT, TABLE_FORMAT_VERSION)?;
    let correlation_nm = correlation_nm(doc)?;
    let mut kernels = BTreeMap::new();
    for (name, k) in doc.req("kernels")?.entries()? {
        let (savgol, mode_stated) = parse_sg(&k.req("savgol")?)?;
        let clip = k.req("absorbance_clip_min")?.f64()?;
        if !(clip > 0.0) {
            return Err(k.error("absorbance_clip_min must be > 0"));
        }
        kernels.insert(
            name,
            Kernel {
                absorbance_clip_min: clip,
                savgol,
                mode_stated,
                scale: k.req("scale")?.f64()?,
            },
        );
    }
    let mut projections = BTreeMap::new();
    if let Some(pj) = doc.opt("projections") {
        for (name, p) in pj.entries()? {
            let window_nm = p.req("window_nm")?.range(false)?;
            let kernel = p.req("kernel")?.str()?.to_string();
            if !kernels.contains_key(&kernel) {
                return Err(p.error(format!("projection {name}: unknown kernel {kernel}")));
            }
            let n = ((window_nm.1 - window_nm.0).round() as usize) + 1;
            let direction = p.req("direction")?.vec_f64_len(n)?;
            let centre = p.req("centre")?.vec_f64_len(n)?;
            projections.insert(
                name,
                Projection {
                    window_nm,
                    kernel,
                    direction,
                    centre,
                },
            );
        }
    }
    let mut bands = BTreeMap::new();
    let bl = doc.req("bands")?;
    if bl.entries()?.is_empty() {
        return Err(bl.error("at least one band required"));
    }
    for (name, b) in bl.entries()? {
        let kernel = b.req("kernel")?.str()?.to_string();
        if !kernels.contains_key(&kernel) {
            return Err(b.error(format!("band {name}: unknown kernel {kernel}")));
        }
        let readability = match b.opt("readability") {
            None => None,
            Some(r) => {
                if r.has("gain_E_per_N2") {
                    return Err(r.error(format!(
                        "band {name}: per-transfer gains are format 1 (format 2 computes them)"
                    )));
                }
                let w = r.req("noise_window_nm")?.range(false)?;
                let envelope_factor = match r.opt("envelope_factor") {
                    None => 1.0,
                    Some(f) => {
                        let x = f.f64()?;
                        if !(x > 0.0) {
                            return Err(f.error(format!(
                                "band {name}: envelope_factor must be a positive finite number"
                            )));
                        }
                        x
                    }
                };
                Some(Readability {
                    noise_window_nm: w,
                    envelope_factor,
                })
            }
        };
        let weight = b.opt("weight").map(|x| x.f64()).transpose()?;
        if weight.is_some_and(|w| !(w > 0.0)) {
            return Err(b.error("weight must be > 0"));
        }
        let centre_nm = b.req("centre_nm")?.f64()?;
        let half_width_nm = b.req("half_width_nm")?.u64()?;
        let projection = b
            .opt("projection")
            .map(|x| x.str().map(str::to_string))
            .transpose()?;
        if let Some(pn) = &projection {
            let pj = projections
                .get(pn)
                .ok_or_else(|| b.error(format!("band {name}: unknown projection {pn}")))?;
            let h = half_width_nm as f64;
            if pj.kernel != kernel
                || centre_nm - h < pj.window_nm.0
                || centre_nm + h > pj.window_nm.1
            {
                return Err(b.error(format!(
                    "band {name}: projection {pn} does not cover the band or uses another kernel"
                )));
            }
        }
        let u_sign = match b.opt("u_sign") {
            None => 1.0,
            Some(x) => {
                let v = x.f64()?;
                if v != 1.0 && v != -1.0 {
                    return Err(x.error("u_sign must be -1 or 1"));
                }
                v
            }
        };
        bands.insert(
            name.clone(),
            Band {
                centre_nm,
                half_width_nm,
                kernel,
                detector: b
                    .req("detector")?
                    .one_of(&["VNIR", "SWIR1", "SWIR2"])?
                    .to_string(),
                roles: b.req("roles")?.vec_str()?,
                weight,
                faint_u: b.opt("faint_u").map(|x| x.f64()).transpose()?,
                noise_window_nm: b
                    .opt("noise_window_nm")
                    .map(|x| x.range(false))
                    .transpose()?,
                readability,
                projection,
                u_sign,
            },
        );
    }
    Ok(Bands {
        header,
        kernels,
        bands,
        projections,
        correlation_nm,
    })
}

impl Bands {
    /// Band readings r_b on a stream (one kernel pass per kernel used).
    pub fn readings(&self, s: &Spectrum) -> Result<BTreeMap<String, f64>, preprocess::OpError> {
        let mut cache: BTreeMap<&str, Spectrum> = BTreeMap::new();
        let mut out = BTreeMap::new();
        for (name, b) in &self.bands {
            if !cache.contains_key(b.kernel.as_str()) {
                let e = self.kernels[&b.kernel].apply(s)?;
                cache.insert(b.kernel.as_str(), e);
            }
            let e = &cache[b.kernel.as_str()];
            let projected;
            let ex: &[f64] = match b.projection.as_ref().and_then(|p| self.projections.get(p)) {
                Some(pj) => {
                    projected = pj.apply(&e.x, &e.wl, self.kernels[&b.kernel].scale);
                    &projected
                }
                None => &e.x,
            };
            let h = b.half_width_nm as f64;
            let v: Vec<f64> =
                e.wl.iter()
                    .zip(ex)
                    .filter(|(w, _)| **w >= b.centre_nm - h - 1e-9 && **w <= b.centre_nm + h + 1e-9)
                    .map(|(_, x)| *x)
                    .collect();
            out.insert(
                name.clone(),
                if v.is_empty() { f64::NAN } else { np_mean(&v) },
            );
        }
        Ok(out)
    }

    /// SD_E of a band read on a stream made with this correction (None = the scan as measured), N2 from the
    /// AS-MEASURED scan: (SD_E, status). The status is "assessed" or why the noise is unknown; an unknown or
    /// non-finite SD is +infinity (unreadable unless strong: fail safe). None for a band without a readability block.
    pub fn readability_sd(
        &self,
        cache: Option<&GainCache>,
        band: &str,
        wl: &[f64],
        joins: &[f64],
        corr: Option<Correction>,
        n2_of: &mut dyn FnMut(f64, f64) -> f64,
    ) -> Option<(f64, String)> {
        self.bands.get(band)?.readability.as_ref()?;
        let g = match cache {
            Some(c) => c.band_gain(self, band, wl, joins, corr),
            None => gains::band_gain(self, band, wl, joins, corr),
        };
        Some(match g {
            Err(e) => (f64::INFINITY, e.0),
            Ok(((lo, hi), g)) => {
                let v = g * n2_of(lo, hi);
                if v.is_finite() {
                    (v, "assessed".to_string())
                } else {
                    (f64::INFINITY, NOISE_UNKNOWN.to_string())
                }
            }
        })
    }
}

/// One recorded noise term: % collagen per unit N2 in a window (a non-linear model's gain).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Term {
    pub window_nm: (f64, f64),
    pub gain: f64,
}

/// Per-model settings of the noise table.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModelNoise {
    /// false: the implied SD is reported, but B6 is not applied to this reader.
    pub b6: bool,
    /// The recorded gain of a non-linear model (for the scan as measured only).
    pub recorded: Option<Vec<Term>>,
}

/// `noise_gains.json` (format_version 2).
#[derive(Debug, Clone)]
pub struct NoiseGains {
    pub header: Header,
    pub provisional: bool,
    /// The noise model's correlation length (nm).
    pub correlation_nm: f64,
    /// One N2 window per detector segment a model kernel may read; a kernel part uses the window in its segment.
    pub model_noise_windows_nm: Vec<(f64, f64)>,
    /// model id -> settings (models not listed: B6 applies, gains computed).
    pub models: BTreeMap<String, ModelNoise>,
    /// consensus model id -> factor (SD = factor x median of the component SDs).
    pub consensus: BTreeMap<String, f64>,
}

pub fn parse_noise_gains(doc: &Node) -> PResult<NoiseGains> {
    let header = parse_header_v(doc, NOISE_GAINS_FORMAT, TABLE_FORMAT_VERSION)?;
    let provisional = doc.req("provisional")?.bool()?;
    let correlation_nm = correlation_nm(doc)?;
    let wn = doc.req("model_noise_windows_nm")?;
    let mut model_noise_windows_nm = Vec::new();
    for w in wn.arr()? {
        model_noise_windows_nm.push(w.range(false)?);
    }
    if model_noise_windows_nm.is_empty() {
        return Err(wn.error("at least one window"));
    }
    let mut models = BTreeMap::new();
    if let Some(ms) = doc.opt("models") {
        for (mid, m) in ms.entries()? {
            let b6 = m.opt("b6").map(|b| b.bool()).transpose()?.unwrap_or(true);
            let recorded = match m.opt("recorded") {
                None => None,
                Some(r) => {
                    let terms = r.req("terms")?.arr()?;
                    if terms.is_empty() {
                        return Err(r.error("at least one term required"));
                    }
                    let mut out = Vec::new();
                    for t in &terms {
                        let gain = t.req("gain")?.f64()?;
                        let w = t.req("window_nm")?.range(false)?;
                        if !(gain > 0.0) {
                            return Err(t.error(format!("{mid}: gain must be > 0")));
                        }
                        out.push(Term { window_nm: w, gain });
                    }
                    Some(out)
                }
            };
            models.insert(mid, ModelNoise { b6, recorded });
        }
    }
    let mut consensus = BTreeMap::new();
    if let Some(c) = doc.opt("consensus") {
        for (cid, e) in c.entries()? {
            let factor = e.req("factor")?.f64()?;
            if !(factor > 0.0) {
                return Err(e.error(format!("consensus {cid}: bad factor")));
            }
            consensus.insert(cid, factor);
        }
    }
    Ok(NoiseGains {
        header,
        provisional,
        correlation_nm,
        model_noise_windows_nm,
        models,
        consensus,
    })
}

/// A model's implied SD on one scan through one correction.
#[derive(Debug, Clone, PartialEq)]
pub struct ImpliedSd {
    /// wt%; None = not assessed (see `status`).
    pub sd: Option<f64>,
    /// A consensus: each component's SD by name, in file order (None for a regression model).
    pub component_sd: Option<Vec<(String, Option<f64>)>>,
    /// "assessed" or why not.
    pub status: String,
    /// false: B6 is not applied to this reader (the SD is still reported).
    pub b6: bool,
}

impl NoiseGains {
    /// The implied SD (wt%) of a model reading through a correction (None = the scan as measured); N2 from the
    /// AS-MEASURED scan (`n2_of` returns NaN when undefined).
    pub fn implied_sd(
        &self,
        cache: Option<&GainCache>,
        m: &Model,
        wl: &[f64],
        joins: &[f64],
        corr: Option<Correction>,
        n2_of: &mut dyn FnMut(f64, f64) -> f64,
    ) -> ImpliedSd {
        let id = m.header.id.as_str();
        let b6 = self.models.get(id).is_none_or(|s| s.b6);
        let names: Vec<String> = match &m.body {
            crate::model::Body::Consensus { components } => {
                components.iter().map(|c| c.name.clone()).collect()
            }
            _ => Vec::new(),
        };
        let cons = m.kind == ModelKind::Consensus;
        let mut out = ImpliedSd {
            sd: None,
            component_sd: cons.then(|| names.iter().map(|n| (n.clone(), None)).collect()),
            status: String::new(),
            b6,
        };
        let terms = match cache {
            Some(c) => c.model_terms(self, m, wl, joins, corr),
            None => gains::model_terms(self, m, wl, joins, corr),
        };
        let terms = match terms {
            Ok(t) => t,
            Err(e) => {
                out.status = e.0;
                return out;
            }
        };
        let sds: Vec<(Option<String>, Option<f64>)> = terms
            .iter()
            .map(|(n, t)| (n.clone(), gains::rss(t, n2_of)))
            .collect();
        if cons {
            let comp: Vec<(String, Option<f64>)> = names
                .iter()
                .map(|n| {
                    let v = sds
                        .iter()
                        .find(|(k, _)| k.as_deref() == Some(n.as_str()))
                        .and_then(|(_, v)| *v);
                    (n.clone(), v)
                })
                .collect();
            match self.consensus.get(id) {
                None => out.status = "not assessed: no noise factor for this consensus".into(),
                Some(_) if comp.iter().any(|(_, v)| v.is_none()) => {
                    out.status = NOISE_UNKNOWN.into()
                }
                Some(f) => {
                    let v: Vec<f64> = comp.iter().filter_map(|(_, v)| *v).collect();
                    out.sd = Some(f * crate::n2::median(&v));
                    out.status = "assessed".into();
                }
            }
            out.component_sd = Some(comp);
            return out;
        }
        match sds.first().and_then(|(_, v)| *v) {
            Some(v) => {
                out.sd = Some(v);
                out.status = "assessed".into();
            }
            None => out.status = NOISE_UNKNOWN.into(),
        }
        out
    }
}

/// The instrument registry.
#[derive(Debug, Clone)]
pub struct Instruments {
    pub header: Header,
    pub default_class: String,
    /// class -> (display name, serials)
    pub classes: BTreeMap<String, (String, Vec<u64>)>,
    /// Template of the gentle mismatch note ({serial}, {other}, {current}), if the file gives one.
    pub mismatch_note: Option<String>,
    /// class -> inclusive range of the header's SWIR1 and SWIR2 gains that suggests it (`header_hint.swir_gain`).
    /// A hint for an unlisted serial only: it presets the switch, never overrides it.
    pub swir_gain_hint: BTreeMap<String, (u64, u64)>,
    /// file format ("as7", "legacy") -> the class of the instrument that writes it (`header_hint.file_format`), unless
    /// the file's detector joins say otherwise (`header_hint.unless_joins_nm`): such a file is scored as that class
    /// whatever the LabSpec switch says (DECISIONS 137). See [`Instruments::class_of_file`].
    pub format_class: BTreeMap<String, FormatClass>,
}

/// The class a file format names (`header_hint.file_format`) and the detector joins that exclude a file of that format
/// from it (`header_hint.unless_joins_nm`). A format is only ASD's file version: as7 is written by the Trek-type
/// handheld and may be written by a LabSpec 4 on older ASD software, which the joins tell apart (1785 vs 1800 nm).
#[derive(Debug, Clone, PartialEq)]
pub struct FormatClass {
    pub class: String,
    /// A file whose joins, as found in it, are exactly these is not this class's (it is read as a LabSpec 4 file).
    pub unless_joins_nm: Option<Vec<f64>>,
}

/// The file formats the reader names (`input.format`): as8, as7 and legacy v1 reflectance (ASD file versions, not
/// instruments: [`Instruments::class_of_file`] also reads the detector joins).
pub const FILE_FORMATS: [&str; 3] = ["as8", "as7", "legacy"];

pub fn parse_instruments(doc: &Node) -> PResult<Instruments> {
    let header = parse_header(doc, INSTRUMENT_FORMAT)?;
    let default_class = doc.req("default_class")?.str()?.to_string();
    let mut classes = BTreeMap::new();
    let mut seen: BTreeMap<u64, String> = BTreeMap::new();
    let mut swir_gain_hint: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let mut format_class: BTreeMap<String, FormatClass> = BTreeMap::new();
    let cl = doc.req("classes")?;
    for (cls, v) in cl.entries()? {
        if let Some(h) = v.opt("header_hint") {
            let unless = h
                .opt("unless_joins_nm")
                .map(|u| {
                    let j = u.vec_f64()?;
                    if j.is_empty() {
                        return Err(u.error("unless_joins_nm needs at least one join"));
                    }
                    Ok(j)
                })
                .transpose()?;
            match h.opt("file_format") {
                Some(f) => {
                    for x in f.arr()? {
                        let fmt = x.one_of(&FILE_FORMATS)?.to_string();
                        let fc = FormatClass {
                            class: cls.clone(),
                            unless_joins_nm: unless.clone(),
                        };
                        if let Some(other) = format_class.insert(fmt.clone(), fc) {
                            return Err(x.error(format!(
                                "file format {fmt:?} named by two classes ({}, {cls})",
                                other.class
                            )));
                        }
                    }
                }
                None if unless.is_some() => {
                    return Err(h.error("unless_joins_nm needs file_format"));
                }
                None => {}
            }
        }
        if let Some(g) = v.opt("header_hint").and_then(|h| h.opt("swir_gain")) {
            let r = g.arr()?;
            if r.len() != 2 {
                return Err(g.error("swir_gain must be [min, max]"));
            }
            let (lo, hi) = (r[0].u64()?, r[1].u64()?);
            if lo > hi {
                return Err(g.error("swir_gain min is above max"));
            }
            if let Some((other, _)) = swir_gain_hint
                .iter()
                .find(|(_, (a, b))| lo <= *b && *a <= hi)
            {
                return Err(g.error(format!("swir_gain range overlaps another class ({other})")));
            }
            swir_gain_hint.insert(cls.clone(), (lo, hi));
        }
        let display = v.req("display_name")?.str()?.to_string();
        let mut serials = Vec::new();
        for s in v.req("serials")?.arr()? {
            let n = s.u64()?;
            if let Some(other) = seen.insert(n, cls.clone()) {
                return Err(s.error(format!(
                    "serial {n} listed for two classes ({other}, {cls})"
                )));
            }
            serials.push(n);
        }
        classes.insert(cls, (display, serials));
    }
    if classes.is_empty() {
        return Err(cl.error("at least one class required"));
    }
    if !classes.contains_key(&default_class) {
        return Err(PluginError::schema("default_class is not a listed class"));
    }
    let mismatch_note = doc
        .opt("switch")
        .and_then(|s| s.opt("mismatch_note"))
        .map(|m| m.str().map(str::to_string))
        .transpose()?;
    Ok(Instruments {
        header,
        default_class,
        classes,
        mismatch_note,
        swir_gain_hint,
        format_class,
    })
}

impl Instruments {
    /// The class a serial is listed under, if any (it only PRESETS the switch).
    pub fn class_of_serial(&self, serial: u64) -> Option<&str> {
        self.classes
            .iter()
            .find(|(_, (_, s))| s.contains(&serial))
            .map(|(c, _)| c.as_str())
    }

    /// The class a file's header SWIR gains suggest (both gains inside one class's `header_hint.swir_gain`), if
    /// any. A heuristic from the detector electronics, for unlisted serials: it presets the switch, nothing more.
    pub fn class_of_swir_gains(&self, swir1: u64, swir2: u64) -> Option<&str> {
        self.swir_gain_hint
            .iter()
            .find(|(_, (lo, hi))| (*lo..=*hi).contains(&swir1) && (*lo..=*hi).contains(&swir2))
            .map(|(c, _)| c.as_str())
    }

    /// The gentle note when a file's serial is listed under another class than the switch (None if it agrees
    /// or the serial is not listed). The switch always wins (DECISIONS 50).
    pub fn mismatch(&self, serial: u64, current_class: &str) -> Option<String> {
        let other = self.class_of_serial(serial)?;
        if other == current_class {
            return None;
        }
        let name = |c: &str| self.display_name(c).unwrap_or(c).to_string();
        let t = self.mismatch_note.clone().unwrap_or_else(|| {
            "This file's instrument (serial {serial}) is listed as {other}. Results use the {current} setting."
                .to_string()
        });
        Some(
            t.replace("{serial}", &serial.to_string())
                .replace("{other}", &name(other))
                .replace("{current}", &name(current_class)),
        )
    }

    pub fn display_name(&self, class: &str) -> Option<&str> {
        self.classes.get(class).map(|(d, _)| d.as_str())
    }

    /// The class of the instrument that wrote a file, from its format and the detector joins found in it (`None` when
    /// the reader fell back to the default joins, [`crate::scan::Scan::found_joins`]): the class the format names
    /// (`header_hint.file_format`) unless the joins are exactly that class's `header_hint.unless_joins_nm`. `None`: a
    /// LabSpec 4 file, whose class the Standard / High-res switch decides. Never decided by the format alone: as7 is
    /// ASD's file version 7, which a LabSpec 4 on older ASD software may write (joins 1000/1800, as on as8).
    pub fn class_of_file(&self, format: &str, joins: Option<&[f64]>) -> Option<&str> {
        let fc = self.format_class.get(format)?;
        match (&fc.unless_joins_nm, joins) {
            (Some(u), Some(j)) if u.as_slice() == j => None,
            _ => Some(fc.class.as_str()),
        }
    }

    /// Whether the registry lists this class.
    pub fn has_class(&self, class: &str) -> bool {
        self.classes.contains_key(class)
    }
}

/// A correction named in a golden block: its operators and instrument noise.
pub struct GoldenCorrection {
    pub ops: Vec<Op>,
    pub noise: InstrumentNoise,
}

/// The named corrections of a golden block (`corrections`: an operator list and optional instrument_noise per name).
pub fn golden_corrections(golden: &Node) -> PResult<BTreeMap<String, GoldenCorrection>> {
    let mut out = BTreeMap::new();
    let Some(cs) = golden.opt("corrections") else {
        return Ok(out);
    };
    for (name, c) in cs.entries()? {
        if name == NO_TRANSFER {
            return Err(c.error("a golden correction cannot be named \"none\""));
        }
        let op = c.req("operator")?;
        if op.arr()?.is_empty() {
            return Err(op.error("needs at least one operator"));
        }
        let ops = preprocess::parse_chain(op.v)?;
        let noise = match c.opt("instrument_noise") {
            None => InstrumentNoise::default(),
            Some(n) => parse_instrument_noise(&n)?,
        };
        out.insert(name, GoldenCorrection { ops, noise });
    }
    Ok(out)
}

/// Goldens of bands.json / noise_gains.json. Each case: spectrum_sha256, correction ("none" or a name in the golden
/// block's `corrections`: an operator list and optional instrument_noise), optional splices_nm (default 1000/1800),
/// expected {name: value} with names `E:<band>`, `SD_E:<band>` (bands) or `N2:<lo>-<hi>`, `SD:<model id>` (noise
/// gains; the models by file SHA-256 in the golden block's `models`, resolved in `models_by_sha`: every loaded model
/// file, any folder).
pub fn run_table_goldens(
    doc: &Node,
    table: TableRef,
    gs: &GoldenSpectra,
    models_by_sha: &BTreeMap<String, &Model>,
) -> PResult<GoldenRun> {
    let golden = doc.req("golden")?;
    let tol: Tolerance = tolerance(&golden)?;
    let cs = cases(&golden)?;
    let corrections = golden_corrections(&golden)?;
    let mut models: BTreeMap<String, &Model> = BTreeMap::new();
    if let (TableRef::Noise(_), Some(ms)) = (table, golden.opt("models")) {
        for (mid, sha) in ms.entries()? {
            let sha = sha.str()?;
            let m = models_by_sha.get(sha).ok_or_else(|| {
                PluginError::golden(format!(
                    "golden: model {mid} (sha256 {}) is not loaded",
                    super::short(sha, 12)
                ))
            })?;
            if m.header.id != mid {
                return Err(PluginError::golden(format!(
                    "golden: the file with sha256 {} is {}, not {mid}",
                    super::short(sha, 12),
                    m.header.id
                )));
            }
            models.insert(mid, m);
        }
    }
    let mut run = GoldenRun::empty(tol);
    let mut covered: std::collections::BTreeSet<String> = Default::default();
    for (i, c) in cs.iter().enumerate() {
        let name = c
            .opt("name")
            .and_then(|n| n.v.as_str().map(str::to_string))
            .unwrap_or_else(|| format!("case {i}"));
        let sha = c.req("spectrum_sha256")?.str()?;
        let g = gs.spectra.get(sha).ok_or_else(|| {
            PluginError::golden(format!(
                "golden case {name}: spectrum not in the golden spectra file"
            ))
        })?;
        let raw = Spectrum::new(g.wl.clone(), g.x.clone());
        let joins = match c.opt("splices_nm") {
            None => vec![1000.0, 1800.0],
            Some(j) => j.vec_f64()?,
        };
        let cname = match c.opt("correction") {
            None => NO_TRANSFER.to_string(),
            Some(x) => x.str()?.to_string(),
        };
        let gc = if cname == NO_TRANSFER {
            None
        } else {
            Some(corrections.get(&cname).ok_or_else(|| {
                PluginError::golden(format!("golden case {name}: unknown correction {cname:?}"))
            })?)
        };
        let corr = gc.map(|x| Correction {
            ops: &x.ops,
            noise: &x.noise,
            key: &cname,
        });
        let exp = match c.opt("expected") {
            Some(e) if e.v.is_object() => e.entries()?,
            _ => Vec::new(),
        };
        if exp.is_empty() {
            return Err(PluginError::golden(format!(
                "golden case {name}: empty expected block"
            )));
        }
        let mut n2_of = |lo: f64, hi: f64| n2(&raw.x, &raw.wl, lo, hi).unwrap_or(f64::NAN);
        let readings = match table {
            TableRef::Bands(b) => {
                let stream = match gc {
                    None => raw.clone(),
                    Some(x) => preprocess::run_chain(&x.ops, &raw, &joins).map_err(|e| {
                        PluginError::golden(format!("golden case {name}: correction {cname}: {e}"))
                    })?,
                };
                Some(
                    b.readings(&stream)
                        .map_err(|e| PluginError::golden(format!("golden case {name}: {e}")))?,
                )
            }
            TableRef::Noise(_) => None,
        };
        for (k, w) in &exp {
            let want = w.f64().map_err(|_| {
                PluginError::golden(format!("golden case {name}: {k} must be a finite number"))
            })?;
            let (kind, what) = k.split_once(':').unwrap_or((k.as_str(), ""));
            let got = match (table, kind) {
                (TableRef::Bands(b), "E") => {
                    if !b.bands.contains_key(what) {
                        return Err(PluginError::golden(format!(
                            "golden case {name}: unknown band {what}"
                        )));
                    }
                    readings.as_ref().and_then(|r| r.get(what).copied())
                }
                (TableRef::Bands(b), "SD_E") => {
                    if !b.bands.contains_key(what) {
                        return Err(PluginError::golden(format!(
                            "golden case {name}: unknown band {what}"
                        )));
                    }
                    covered.insert(what.to_string());
                    b.readability_sd(None, what, &raw.wl, &joins, corr, &mut n2_of)
                        .map(|(v, _)| v)
                }
                (TableRef::Noise(_), "N2") => {
                    let (lo, hi) = what
                        .split_once('-')
                        .and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?)))
                        .ok_or_else(|| {
                            PluginError::golden(format!("golden case {name}: bad N2 window {what}"))
                        })?;
                    n2(&raw.x, &raw.wl, lo, hi)
                }
                (TableRef::Noise(ng), "SD") => {
                    let m = models.get(what).ok_or_else(|| {
                        PluginError::golden(format!(
                            "golden case {name}: model {what} is not in the golden block's models"
                        ))
                    })?;
                    covered.insert(what.to_string());
                    ng.implied_sd(None, m, &raw.wl, &joins, corr, &mut n2_of).sd
                }
                _ => {
                    return Err(PluginError::golden(format!(
                        "golden case {name}: unknown expected output {k}"
                    )))
                }
            };
            run.scalar(&name, k, got, want);
        }
    }
    if run.checks == 0 {
        return Err(PluginError::golden("golden: zero comparisons"));
    }
    // Codex Phase 2 MEDIUM 6: the goldens must exercise every computed output the table governs
    let need: std::collections::BTreeSet<String> = match table {
        TableRef::Bands(b) => b
            .bands
            .iter()
            .filter(|(_, x)| x.readability.is_some())
            .map(|(k, _)| k.clone())
            .collect(),
        TableRef::Noise(ng) => models
            .keys()
            .cloned()
            .chain(ng.consensus.keys().cloned())
            .chain(
                ng.models
                    .iter()
                    .filter(|(_, s)| s.recorded.is_some())
                    .map(|(k, _)| k.clone()),
            )
            .collect(),
    };
    let missing: Vec<String> = need.difference(&covered).cloned().collect();
    if !missing.is_empty() {
        return Err(PluginError::golden(format!(
            "golden: no case tests {}",
            missing.join(", ")
        )));
    }
    Ok(run)
}

/// Which table a golden block belongs to.
#[derive(Clone, Copy)]
pub enum TableRef<'a> {
    Bands(&'a Bands),
    Noise(&'a NoiseGains),
}

/// SG mode of a kernel, as text (for reports).
pub fn mode_name(m: SgMode) -> &'static str {
    match m {
        SgMode::Interp => "interp",
        SgMode::Mirror => "mirror",
        SgMode::Nearest => "nearest",
        SgMode::Constant => "constant",
        SgMode::Wrap => "wrap",
        SgMode::ZeroEdges => "zero_edges",
        SgMode::Valid => "valid",
    }
}
