//! Shared-table plug-ins (PLAN section 4; spyder_ref `tables`): `bands.json`, `noise_gains.json` and the
//! instrument registry.
//!
//! * bands: band reading r_b = mean over centre +/- half_width of E = scale x SG derivative of
//!   log10(1/max(R, clip)) (kernel per band); readability SD_E = gain_E_per_N2[transfer key] x N2(noise window)
//!   on the AS-MEASURED scan.
//! * noise gains: implied SD (wt%) of a collagen reading = sqrt(sum (gain x N2(window))^2) per model and transfer
//!   key; a consensus = factor x median of its component SDs.
//! * Transfer key = `"none"` or the SHA-256 of the transfer FILE applied to the stream; a key with no entry is
//!   "not assessed", never a guess.
//! * instruments: the classes and serial lists that PRESET the user's Standard / High-res switch (DECISIONS 50).

use std::collections::BTreeMap;

use serde::Serialize;

use super::golden::{cases, tolerance, GoldenRun, GoldenSpectra, Tolerance};
use super::{
    is_sha256_hex, parse_header, Header, Node, PResult, PluginError, ScanContext, BANDS_FORMAT,
    INSTRUMENT_FORMAT, NOISE_GAINS_FORMAT,
};
use crate::n2::n2;
use crate::numsum::np_mean;
use crate::preprocess::{self, SgMode, SgParams, Spectrum};
use crate::transfer::Transfer;

/// The key of a stream: no transfer, or the SHA-256 of the transfer file applied.
pub const NO_TRANSFER: &str = "none";

fn transfer_key_ok(k: &str) -> bool {
    k == NO_TRANSFER || is_sha256_hex(k)
}

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

/// Readability of one band.
#[derive(Debug, Clone, PartialEq)]
pub struct Readability {
    pub noise_window_nm: (f64, f64),
    pub gain_e_per_n2: BTreeMap<String, f64>,
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
}

pub fn parse_bands(doc: &Node) -> PResult<Bands> {
    let header = parse_header(doc, BANDS_FORMAT)?;
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
                let w = r.req("noise_window_nm")?.range(false)?;
                let mut g = BTreeMap::new();
                let gn = r.req("gain_E_per_N2")?;
                for (k, v) in gn.entries()? {
                    if !transfer_key_ok(&k) {
                        return Err(gn.error(format!(
                            "transfer key {k:?} must be \"none\" or a file sha256"
                        )));
                    }
                    let x = v.f64()?;
                    if !(x > 0.0) {
                        return Err(v.error(format!(
                            "band {name}: gain for {k} must be a positive finite number"
                        )));
                    }
                    g.insert(k, x);
                }
                if g.is_empty() {
                    return Err(gn.error("at least one gain required"));
                }
                Some(Readability {
                    noise_window_nm: w,
                    gain_e_per_n2: g,
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

    /// SD of a band reading from the as-measured scan, for the transfer key of the stream it is read on;
    /// None = not assessed (no readability block or no gain for this key).
    pub fn readability_sd(&self, band: &str, raw: &Spectrum, key: &str) -> Option<f64> {
        let r = self.bands.get(band)?.readability.as_ref()?;
        let g = r.gain_e_per_n2.get(key)?;
        let (lo, hi) = r.noise_window_nm;
        Some(g * n2(&raw.x, &raw.wl, lo, hi)?)
    }
}

/// One noise term: % collagen per unit N2 in a window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Term {
    pub window_nm: (f64, f64),
    pub gain: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsensusGain {
    pub factor: f64,
    /// component name -> model id
    pub components: BTreeMap<String, String>,
}

/// `noise_gains.json`.
#[derive(Debug, Clone)]
pub struct NoiseGains {
    pub header: Header,
    pub provisional: bool,
    /// model id -> transfer key -> terms
    pub models: BTreeMap<String, BTreeMap<String, Vec<Term>>>,
    pub consensus: BTreeMap<String, ConsensusGain>,
}

pub fn parse_noise_gains(doc: &Node) -> PResult<NoiseGains> {
    let header = parse_header(doc, NOISE_GAINS_FORMAT)?;
    let provisional = doc.req("provisional")?.bool()?;
    let mut models = BTreeMap::new();
    for (mid, m) in doc.req("models")?.entries()? {
        let mut keyed = BTreeMap::new();
        for (k, e) in m.entries()? {
            if !transfer_key_ok(&k) {
                return Err(m.error(format!(
                    "transfer key {k:?} must be \"none\" or a file sha256"
                )));
            }
            let terms = e.req("terms")?.arr()?;
            if terms.is_empty() {
                return Err(e.error("at least one term required"));
            }
            let mut out = Vec::new();
            for t in &terms {
                let gain = t.req("gain")?.f64()?;
                let w = t.req("window_nm")?.range(false)?;
                if !(gain > 0.0) {
                    return Err(t.error(format!("{mid} [{k}]: gain must be > 0")));
                }
                out.push(Term { window_nm: w, gain });
            }
            keyed.insert(k, out);
        }
        models.insert(mid, keyed);
    }
    let mut consensus = BTreeMap::new();
    if let Some(c) = doc.opt("consensus") {
        for (cid, e) in c.entries()? {
            let factor = e.req("factor")?.f64()?;
            let comps: BTreeMap<String, String> = e
                .req("components")?
                .entries()?
                .into_iter()
                .map(|(k, v)| v.str().map(|s| (k, s.to_string())))
                .collect::<PResult<_>>()?;
            if !(factor > 0.0) || comps.len() < 3 || comps.len() % 2 != 1 {
                return Err(e.error(format!("consensus {cid}: bad factor or component count")));
            }
            for mid in comps.values() {
                // Codex Phase 2 HIGH 2: components resolve only to plain model entries (no consensus of
                // consensuses, no cycles): every component must have its own gains and must not be a consensus
                if !models.contains_key(mid) {
                    return Err(e.error(format!("consensus {cid}: component {mid} has no gains")));
                }
                if mid == &cid || c.opt(mid).is_some() {
                    return Err(e.error(format!(
                        "consensus {cid}: component {mid} is itself a consensus (not allowed)"
                    )));
                }
            }
            consensus.insert(
                cid,
                ConsensusGain {
                    factor,
                    components: comps,
                },
            );
        }
    }
    Ok(NoiseGains {
        header,
        provisional,
        models,
        consensus,
    })
}

impl NoiseGains {
    /// Implied SD (wt%) of a model on the as-measured scan for a transfer key; None = not assessed.
    pub fn implied_sd(&self, model_id: &str, raw: &Spectrum, key: &str) -> Option<f64> {
        if let Some(c) = self.consensus.get(model_id) {
            // components are plain models (checked at parse time): no recursion
            let mut sds = Vec::new();
            for mid in c.components.values() {
                sds.push(self.model_sd(mid, raw, key)?);
            }
            return Some(c.factor * crate::n2::median(&sds));
        }
        self.model_sd(model_id, raw, key)
    }

    fn model_sd(&self, model_id: &str, raw: &Spectrum, key: &str) -> Option<f64> {
        let terms = self.models.get(model_id)?.get(key)?;
        let mut s = 0.0;
        for t in terms {
            let v = t.gain * n2(&raw.x, &raw.wl, t.window_nm.0, t.window_nm.1)?;
            s += v * v;
        }
        Some(s.sqrt())
    }
}

impl ConsensusGain {
    /// (component name, model id) pairs.
    pub fn components_ordered(&self) -> impl Iterator<Item = (&String, &String)> {
        self.components.iter()
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
}

pub fn parse_instruments(doc: &Node) -> PResult<Instruments> {
    let header = parse_header(doc, INSTRUMENT_FORMAT)?;
    let default_class = doc.req("default_class")?.str()?.to_string();
    let mut classes = BTreeMap::new();
    let mut seen: BTreeMap<u64, String> = BTreeMap::new();
    let mut swir_gain_hint: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    let cl = doc.req("classes")?;
    for (cls, v) in cl.entries()? {
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
}

fn stream(raw: &Spectrum, key: &str, transfers: &BTreeMap<String, &Transfer>) -> PResult<Spectrum> {
    if key == NO_TRANSFER {
        return Ok(raw.clone());
    }
    let t = transfers.get(key).ok_or_else(|| {
        PluginError::golden(format!(
            "transfer key {} has no transfer file in this folder",
            super::short(key, 12)
        ))
    })?;
    // spyder_ref tables.stream: joins 1000/1800, the transfer's source class
    let ctx = ScanContext::new(vec![1000.0, 1800.0], &t.source_class, None);
    t.apply(raw, &ctx)
        .map_err(|e| PluginError::golden(format!("transfer {}: {e}", t.key())))
}

/// Goldens of bands.json / noise_gains.json. Each case: spectrum_sha256, transfer_key, expected {name: value}
/// with names `E:<band>`, `SD_E:<band>` (bands) or `N2:<lo>-<hi>`, `SD:<model id>` (noise gains).
/// `transfers` = the transfer files of the same folder, keyed by file SHA-256.
pub fn run_table_goldens(
    doc: &Node,
    table: TableRef,
    gs: &GoldenSpectra,
    transfers: &BTreeMap<String, &Transfer>,
) -> PResult<GoldenRun> {
    let golden = doc.req("golden")?;
    let tol: Tolerance = tolerance(&golden)?;
    let cs = cases(&golden)?;
    let mut run = GoldenRun {
        checks: 0,
        failures: Vec::new(),
        worst_ratio: 0.0,
        tolerance: tol,
    };
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
        let key = c.req("transfer_key")?.str()?;
        if !transfer_key_ok(key) {
            return Err(PluginError::golden(format!(
                "golden case {name}: transfer_key must be \"none\" or a file sha256"
            )));
        }
        let exp = match c.opt("expected") {
            Some(e) if e.v.is_object() => e.entries()?,
            _ => Vec::new(),
        };
        if exp.is_empty() {
            return Err(PluginError::golden(format!(
                "golden case {name}: empty expected block"
            )));
        }
        let readings = match table {
            TableRef::Bands(b) => Some(
                b.readings(&stream(&raw, key, transfers)?)
                    .map_err(|e| PluginError::golden(format!("golden case {name}: {e}")))?,
            ),
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
                (TableRef::Bands(b), "SD_E") => b.readability_sd(what, &raw, key),
                (TableRef::Noise(_), "N2") => {
                    let (lo, hi) = what
                        .split_once('-')
                        .and_then(|(a, b)| Some((a.parse::<f64>().ok()?, b.parse::<f64>().ok()?)))
                        .ok_or_else(|| {
                            PluginError::golden(format!("golden case {name}: bad N2 window {what}"))
                        })?;
                    n2(&raw.x, &raw.wl, lo, hi)
                }
                (TableRef::Noise(ng), "SD") => ng.implied_sd(what, &raw, key),
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
    // Codex Phase 2 MEDIUM 6: the goldens must exercise every gain the table declares
    let mut covered: std::collections::BTreeSet<(String, String)> = Default::default();
    for c in &cs {
        let key = c.req("transfer_key")?.str()?.to_string();
        if let Some(e) = c.opt("expected") {
            for (k, _) in e.entries()? {
                if let Some(id) = k.strip_prefix("SD:").or_else(|| k.strip_prefix("SD_E:")) {
                    covered.insert((id.to_string(), key.clone()));
                }
            }
        }
    }
    let mut needed: Vec<(String, String)> = Vec::new();
    match table {
        TableRef::Noise(ng) => {
            for (mid, keyed) in &ng.models {
                for k in keyed.keys() {
                    needed.push((mid.clone(), k.clone()));
                }
            }
            for (cid, c) in &ng.consensus {
                let mut keys: Option<std::collections::BTreeSet<&String>> = None;
                for mid in c.components.values() {
                    let ks: std::collections::BTreeSet<&String> = ng
                        .models
                        .get(mid)
                        .map(|m| m.keys().collect())
                        .unwrap_or_default();
                    keys = Some(match keys {
                        None => ks,
                        Some(prev) => prev.intersection(&ks).copied().collect(),
                    });
                }
                for k in keys.unwrap_or_default() {
                    needed.push((cid.clone(), k.clone()));
                }
            }
        }
        TableRef::Bands(b) => {
            for (bid, band) in &b.bands {
                if let Some(r) = &band.readability {
                    for k in r.gain_e_per_n2.keys() {
                        needed.push((bid.clone(), k.clone()));
                    }
                }
            }
        }
    }
    let missing: Vec<String> = needed
        .into_iter()
        .filter(|n| !covered.contains(n))
        .map(|(id, k)| format!("{id} [{}]", super::short(&k, 12)))
        .collect();
    if !missing.is_empty() {
        return Err(PluginError::golden(format!(
            "golden: no case tests the gains of {}",
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
