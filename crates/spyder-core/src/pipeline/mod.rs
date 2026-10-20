//! The full per-scan pipeline (PLAN v3.2 Steps 1-10 in the Step 3 order of operations), a port of the
//! Python oracle `reference/oracle` (pipeline.py, checks.py, verdict.py, export.py). Its result record has the
//! oracle's structure exactly, so that the end-to-end goldens and the private parity compare field by field.
//!
//!  1. read the file; raw acquisition statistics (N2 per window, R range, splice steps) on the AS-MEASURED scan;
//!  2. the instrument class is an INPUT (the user's Standard / High-res switch for LabSpec files; a file another
//!     instrument wrote, named by its format in the instrument registry, is scored as that instrument's class); a
//!     known serial adds a gentle note when it disagrees; then the models, the stream correction and the profiles. The
//!     noise policies (per-sign gates, long-wave, soft tier, second opinion, S1_R2 shown) key on the correction applied
//!     and on whether the stream is the native standard-resolution scan (as measured, or through a near-uncorrected
//!     correction such as a standard-resolution unit's own file), never on class names (DECISIONS 137);
//!  3. noise propagation computed for each model and band from the correction actually applied and the scan's own
//!     detector segments (no lookup by transfer file);
//!  4. B6, long-wave eligibility, band readability (an unknown band noise is +infinity: the band is unreadable unless
//!     strong, with a "band noise unknown" note);
//!  5. on the standard-resolution-equivalent stream: B9, models, CONS3, evidence, ZooMS pattern, signs, C1;
//!  6. the verdict per analysis type; the CSV row and the analysis manifest ([`export`]).

pub mod checkgold;
pub mod checks;
pub mod export;
pub mod kernels;
pub mod val;
pub mod verdict;

use std::collections::BTreeMap;

use crate::plugins::checks::{CheckParams, EngineCheck};
use crate::plugins::profiles::AnalysisProfile;
use crate::plugins::registry::Registry;
use crate::plugins::tables::{Bands, Instruments, NoiseGains, B6_NOT_APPLIED, NO_TRANSFER};
use crate::plugins::{
    sha256_hex, spectrum_sha256, ScanContext, BANDS_FORMAT, ENGINE_CHECK_FORMAT, INSTRUMENT_FORMAT,
    MODEL_FORMAT, NOISE_GAINS_FORMAT, PROFILE_FORMAT, TRANSFER_FORMAT,
};
use crate::preprocess::Spectrum;
use crate::transfer::{select_target, Selection, Transfer};
use checks::N2Cache;
use checks::Route;
use val::{fin, Obj, V};
use verdict::VerdictInputs;

/// The oracle version whose records this pipeline reproduces.
pub const ORACLE_VERSION: &str = "1.0.0";
/// The analysis types, in record order.
pub const ANALYSES: [&str; 3] = ["radiocarbon", "isotopes", "zooms"];

/// The class and its source (the user's switch), the serial and the scan's joins.
#[derive(Debug, Clone)]
pub struct Context {
    pub instrument_class: String,
    /// "user" (the switch) or "preset" (a serial preset the user accepted).
    pub class_source: String,
    pub serial: Option<u64>,
    pub splices_nm: Vec<f64>,
}

/// Every plug-in the pipeline reads, resolved once from a loaded registry.
pub struct Engine<'r> {
    pub reg: &'r Registry,
    checks: BTreeMap<String, &'r EngineCheck>,
    profiles: BTreeMap<String, &'r AnalysisProfile>,
    bands: &'r Bands,
    gains: &'r NoiseGains,
    instruments: Option<&'r Instruments>,
}

fn dep(reg: &Registry, format: &str, id: &str) -> Option<(String, String)> {
    reg.selected_entry(format, id)
        .map(|e| (e.label(), e.sha256.clone()))
}

impl<'r> Engine<'r> {
    /// Resolve the pipeline's plug-ins; an error names what is missing (the startup-error state).
    pub fn new(reg: &'r Registry) -> Result<Self, String> {
        let checks: BTreeMap<String, &EngineCheck> = reg
            .checks()
            .into_iter()
            .map(|c| (c.check.clone(), c))
            .collect();
        for c in [
            "acquisition",
            "longwave",
            "b9",
            "evidence_levels",
            "zooms_patterns",
            "signs",
            "heat",
            "c1",
        ] {
            if !checks.contains_key(c) {
                return Err(format!("no loaded engine check '{c}'"));
            }
        }
        let profiles: BTreeMap<String, &AnalysisProfile> = reg
            .profiles()
            .into_iter()
            .map(|p| (p.analysis.clone(), p))
            .collect();
        for a in ANALYSES {
            if !profiles.contains_key(a) {
                return Err(format!("no loaded analysis profile '{a}'"));
            }
        }
        let bands = reg.bands().ok_or("no loaded band table (bands.json)")?;
        let gains = reg
            .noise_gains()
            .ok_or("no loaded noise gains (noise_gains.json)")?;
        Ok(Engine {
            reg,
            checks,
            profiles,
            bands,
            gains,
            instruments: reg.instruments(),
        })
    }

    fn cp(&self, name: &str) -> &'r CheckParams {
        &self.checks[name].params
    }

    pub fn profile(&self, analysis: &str) -> Option<&'r AnalysisProfile> {
        self.profiles.get(analysis).copied()
    }

    fn check_dep(&self, name: &str, deps: &mut BTreeMap<String, String>) {
        if let Some(c) = self.checks.get(name) {
            if let Some((k, v)) = dep(self.reg, ENGINE_CHECK_FORMAT, &c.header.id) {
                deps.insert(k, v);
            }
        }
    }

    // ------------------------------------------------------------------ entry points
    /// Analyse an ASD file's bytes (`as8`, `as7` or legacy; the watcher passes immutable snapshots). `name` is the
    /// file name shown.
    pub fn analyse_bytes(&self, bytes: &[u8], name: &str, class: &str, class_source: &str) -> Obj {
        let mut deps = BTreeMap::new();
        self.check_dep("acquisition", &mut deps);
        let meta = match asd_meta(bytes, name) {
            Ok(m) => m,
            Err((reason, meta)) => return self.rejected(&reason, meta, deps),
        };
        let scan = match crate::read::read_bytes(bytes) {
            Ok(s) => s,
            Err(e) => return self.rejected(&e.to_string(), meta, deps),
        };
        if !scan.kind.is_scored() {
            return self.not_scored(meta, deps);
        }
        if let Some(reason) = self.acceptance(&scan) {
            return self.rejected(&reason, meta, deps);
        }
        // a file another instrument wrote (named in the instrument registry by its format and detector joins) is scored
        // as that instrument's class: the Standard / High-res switch chooses between the LabSpec classes only
        // (DECISIONS 137). An as7 file with the LabSpec joins 1000/1800 is a LabSpec 4 file, like as8.
        let (class, class_source) = match self.file_class(&scan) {
            Some(c) if c != class => (c, "file_format"),
            _ => (class, class_source),
        };
        let ctx = Context {
            instrument_class: class.to_string(),
            class_source: class_source.to_string(),
            serial: Some(u64::from(scan.header.serial)),
            splices_nm: scan.splices_nm.clone(),
        };
        self.analyse(
            &scan.wavelengths_nm,
            &scan.reflectance,
            &ctx,
            scan.sample_dn.as_deref(),
            meta,
        )
    }

    /// The class of the instrument that wrote this file, from its format and the detector joins found in it, if the
    /// instrument registry names one ([`crate::plugins::tables::Instruments::class_of_file`]); `None`: a LabSpec 4 file.
    fn file_class(&self, scan: &crate::scan::Scan) -> Option<&'r str> {
        self.instruments
            .and_then(|i| i.class_of_file(scan.format.as_str(), scan.found_joins()))
    }

    /// The record of a file that could not be read from disk (B1, "Rescan").
    pub fn analyse_unreadable(&self, name: &str, reason: &str) -> Obj {
        let mut deps = BTreeMap::new();
        self.check_dep("acquisition", &mut deps);
        self.rejected(reason, Obj::new().with("file", name), deps)
    }

    /// Analyse a reflectance spectrum (no file): the input hash is the spectrum's content hash.
    pub fn analyse_spectrum(&self, wl: &[f64], r: &[f64], ctx: &Context) -> Obj {
        let meta = Obj::new()
            .with("input_sha256", spectrum_sha256(wl, r))
            .with("input_kind", "spectrum");
        self.analyse(wl, r, ctx, None, meta)
    }

    /// The standard-resolution-equivalent stream (Step 4) for DISPLAY: the scan as measured when no transfer
    /// applies (standard-res, or no transfer available), else the transfer the pipeline selects for the
    /// standard class, by the same rule and on the same joins as [`Engine::analyse_bytes`]. Returns the stream
    /// and the transfer key (`id@version`), `None` when the stream is the scan as measured. Computes nothing
    /// the pipeline reads; the charts call it so that what they draw is what the consumers read.
    pub fn standard_stream(
        &self,
        wl: &[f64],
        r_meas: &[f64],
        joins: &[f64],
        class: &str,
        serial: Option<u64>,
    ) -> (Vec<f64>, Option<String>) {
        let transfers = self.reg.transfers();
        let pinned = self.reg.pinned_transfer_ids();
        let sctx = ScanContext::new(joins.to_vec(), class, serial.map(|s| s.to_string()));
        let pick_ctx = ScanContext::new(vec![1000.0, 1800.0], class, serial.map(|s| s.to_string()));
        match select_target(
            crate::plugins::CLASS_STD,
            &[],
            &pick_ctx,
            &transfers,
            &pinned,
        ) {
            Selection::Apply(t) => (transferred(t, wl, r_meas, &sctx), Some(t.key())),
            Selection::NotNeeded | Selection::Missing => (r_meas.to_vec(), None),
        }
    }

    /// The oracle's `asd.acceptance` on a parsed scan (check.acquisition's input matrix; data-driven).
    fn acceptance(&self, scan: &crate::scan::Scan) -> Option<String> {
        let CheckParams::Acquisition {
            input_matrix: m, ..
        } = self.cp("acquisition")
        else {
            return None;
        };
        let h = &scan.header;
        let legacy = scan.format == crate::scan::FileFormat::Legacy;
        if !m.file_magic.contains(&h.version) {
            return Some(format!("unsupported: file version {:?}", h.version));
        }
        if m.data_type_by_magic.get(&h.version) != Some(&u64::from(h.data_type)) {
            return Some(if legacy {
                "unsupported: not a reflectance file".into()
            } else {
                "unsupported: not a raw (DN + white reference) file".into()
            });
        }
        if m.require_dark_corrected_magic.contains(&h.version) && h.dark_corrected == 0 {
            return Some("unsupported: dark correction flag not set".into());
        }
        if usize::from(h.channels) != m.grid_n
            || (f64::from(h.first_wavelength_nm) - m.grid_start_nm).abs() > 1e-6
            || (f64::from(h.wavelength_step_nm) - m.grid_step_nm).abs() > 1e-6
        {
            return Some("unsupported: wavelength grid".into());
        }
        if legacy {
            if scan.reflectance.iter().any(|v| !v.is_finite()) {
                return Some("unsupported: non-finite reflectance values".into());
            }
        } else {
            let (Some(rd), Some(sd)) = (&scan.reference_dn, &scan.sample_dn) else {
                return Some("unsupported: no white-reference block".into());
            };
            let (lo, hi) = m.reference_range_nm;
            let bad_ref = scan
                .wavelengths_nm
                .iter()
                .zip(rd)
                .any(|(w, v)| *w >= lo && *w <= hi && !(v.is_finite() && *v > 0.0));
            if bad_ref {
                return Some("unsupported: white reference not finite and positive".into());
            }
            if sd.iter().any(|v| !v.is_finite()) {
                return Some("unsupported: non-finite sample values".into());
            }
        }
        if !joins_accepted(&scan.splices_nm, &m.join_windows_nm) {
            return Some(format!(
                "unsupported: detector joins {}",
                val::py_json(
                    &V::List(scan.splices_nm.iter().map(|x| V::Num(*x)).collect()),
                    false
                )
            ));
        }
        None
    }

    /// Add the profile's "most promising first" key to a verdict block without a reading (Codex Phase 3
    /// review 5: every verdict has one, so Rescan sorts where the profile puts it).
    fn sorted(&self, analysis: &str, mut v: Obj) -> Obj {
        if let Some(p) = self.profiles.get(analysis) {
            let vd = v
                .get("verdict")
                .and_then(V::as_str)
                .unwrap_or("")
                .to_string();
            let (g, val) = verdict::sort_key(p, &vd, None, None);
            v.set("sort_group", g);
            v.set("sort_value", val);
            if let Some(pen) = verdict::sort_penalty(p, &v) {
                v.set("sort_penalty", pen);
            }
        }
        v
    }

    fn dependencies(deps: BTreeMap<String, String>) -> Obj {
        let mut o = Obj::new();
        for (k, v) in deps {
            o.set(&k, v);
        }
        o
    }

    fn rejected(&self, reason: &str, meta: Obj, deps: BTreeMap<String, String>) -> Obj {
        let mut r = Obj::new()
            .with("oracle_version", ORACLE_VERSION)
            .with("input", meta)
            .with(
                "checks",
                Obj::new().with(
                    "B1",
                    Obj::new()
                        .with("status", "assessed")
                        .with("outcome", "Unusable")
                        .with("reason", reason),
                ),
            );
        let verdict = if reason.starts_with("unsupported") {
            "Unsupported"
        } else {
            "Rescan"
        };
        for a in ANALYSES {
            r.set(a, self.sorted(a, empty_verdict(verdict, "B1")));
        }
        r.set("dependencies", Self::dependencies(deps));
        r
    }

    /// A white-reference save listed by the reader: "reference scan (not scored)" (no verdict).
    fn not_scored(&self, meta: Obj, deps: BTreeMap<String, String>) -> Obj {
        let mut r = Obj::new()
            .with("oracle_version", ORACLE_VERSION)
            .with("input", meta)
            .with(
                "checks",
                Obj::new().with(
                    "B1",
                    Obj::new()
                        .with("status", "assessed")
                        .with("outcome", "Not scored")
                        .with("reason", "reference scan (not scored)"),
                ),
            );
        for a in ANALYSES {
            r.set(a, self.sorted(a, empty_verdict("Not scored", "B1")));
        }
        r.set("dependencies", Self::dependencies(deps));
        r
    }

    // ------------------------------------------------------------------ main
    fn analyse(
        &self,
        wl: &[f64],
        r_meas: &[f64],
        ctx: &Context,
        dn: Option<&[f64]>,
        meta: Obj,
    ) -> Obj {
        let mut deps = BTreeMap::new();
        self.check_dep("acquisition", &mut deps);
        let legacy_input = meta.get("format").and_then(V::as_str) == Some("legacy");
        let cls = ctx.instrument_class.as_str();
        let serial = ctx.serial;
        let joins = &ctx.splices_nm;
        let acqp = self.cp("acquisition");
        let CheckParams::Acquisition {
            input_matrix: im,
            b6_cuts,
            b6_check_if_cut_within_sd,
            ..
        } = acqp
        else {
            return self.rejected("unsupported: no acquisition parameters", meta, deps);
        };
        // ---- Step 1 grid + joins (spectrum input: the matrix parts that apply)
        // Codex Phase 3 review 2: an empty or short spectrum is unsupported input, never a panic
        let okgrid = wl.len() == im.grid_n
            && r_meas.len() == wl.len()
            && !wl.is_empty()
            && (wl[0] - im.grid_start_nm).abs() < 1e-6
            && wl.windows(2).all(|p| {
                ((p[1] - p[0]) - im.grid_step_nm).abs() <= 1e-8 + 1e-5 * im.grid_step_nm.abs()
            });
        let okjoin = joins_accepted(joins, &im.join_windows_nm);
        if !(okgrid && okjoin) {
            let why = if !okgrid {
                "wavelength grid".to_string()
            } else {
                format!(
                    "detector joins {}",
                    val::py_json(&V::List(joins.iter().map(|x| V::Num(*x)).collect()), false)
                )
            };
            return self.rejected(&format!("unsupported: {why}"), meta, deps);
        }
        let mut rec = Obj::new()
            .with("oracle_version", ORACLE_VERSION)
            .with("input", meta)
            .with(
                "instrument",
                Obj::new()
                    .with("class", cls)
                    .with("class_source", ctx.class_source.as_str())
                    .with("serial", serial.map(|s| s as i64)),
            );
        // ---- Step 2: raw acquisition statistics, as measured
        let mut n2c = N2Cache::new(wl, r_meas);
        let mut acq = Obj::new().with(
            "B1",
            Obj::new().with("status", "assessed").with("outcome", "ok"),
        );
        let no_dn = if legacy_input {
            "not assessed: the file stores reflectance only"
        } else {
            "not assessed: no raw DN (spectrum input)"
        };
        acq.update(checks::acquisition(wl, r_meas, acqp, dn, joins, no_dn));
        // the stream's transfer is resolved (picked) first: the per-sign B6b gates are keyed by it (phase 0c); it is
        // applied after the Rescan check below
        let transfers = self.reg.transfers();
        let pinned = self.reg.pinned_transfer_ids();
        let pick_ctx = ScanContext::new(vec![1000.0, 1800.0], cls, serial.map(|s| s.to_string()));
        let selection = select_target(
            crate::plugins::CLASS_STD,
            &[],
            &pick_ctx,
            &transfers,
            &pinned,
        );
        // the noise policies follow the correction applied (DECISIONS 137). native: the standard-resolution scan as
        // measured, or through a near-uncorrected correction (every noise-policy gain within check.acquisition's
        // std_like_within of uncorrected, e.g. a standard-resolution unit's own file): every noise policy then treats
        // the stream exactly like a native standard-resolution scan (Matt, 2026-10-19: LabSpec 4 standard never changes)
        let scorr = match &selection {
            Selection::Apply(t) => Some(t.correction()),
            _ => None,
        };
        let route = Route {
            wl,
            joins,
            corr: scorr,
            cache: Some(&self.reg.gain_cache),
        };
        let near = checks::standard_like(acqp, self.cp("longwave"), self.cp("signs"), &route);
        let native_why = if near {
            Some(checks::NEAR_UNCORRECTED)
        } else if cls == crate::plugins::CLASS_STD && matches!(selection, Selection::NotNeeded) {
            Some(checks::NATIVE_SCAN)
        } else {
            None
        };
        let native = native_why.is_some();
        let (b6b, gates, gated, reduced) = checks::b6b(acqp, &mut n2c, &route, near);
        acq.set("B6b", b6b);
        let bad: Vec<&str> = ["B1", "B2", "B3", "B4"]
            .into_iter()
            .filter(|k| {
                acq.get(k)
                    .and_then(V::as_obj)
                    .and_then(|o| o.get("outcome"))
                    .and_then(V::as_str)
                    == Some("Unusable")
            })
            .collect();
        rec.set("checks", acq);
        // ---- Step 3: class (the switch), serial preset / mismatch, profiles
        let known = serial.and_then(|s| self.instruments.and_then(|i| i.class_of_serial(s)));
        let mut inst_notes = Vec::new();
        if let Some(k) = known {
            if k != cls {
                inst_notes.push(V::Map(
                    Obj::new()
                        .with("key", "serial_class_mismatch")
                        .with("serial_class", k),
                ));
            }
        }
        if let Some(i) = self.instruments {
            if let Some((k, v)) = dep(self.reg, INSTRUMENT_FORMAT, &i.header.id) {
                deps.insert(k, v);
            }
        }
        if let Some(V::Map(o)) = rec.get_mut("instrument") {
            o.set("serial_known_class", known);
            o.set("notes", V::List(inst_notes));
        }
        let mut po = Obj::new();
        for a in ANALYSES {
            let p = self.profiles[a];
            po.set(a, p.header.key());
            if let Some((k, v)) = dep(self.reg, PROFILE_FORMAT, &p.header.id) {
                deps.insert(k, v);
            }
        }
        rec.set("profiles", po);
        if !bad.is_empty() {
            for a in ANALYSES {
                rec.set(a, self.sorted(a, empty_verdict("Rescan", &bad.join("+"))));
            }
            rec.set("n2", n2c.table());
            rec.set("dependencies", Self::dependencies(deps));
            return rec;
        }
        // the stream transfer (Step 4): high-res -> standard-res
        let sctx = ScanContext::new(joins.clone(), cls, serial.map(|s| s.to_string()));
        let mut stream_cache: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut apply = |t: &Transfer, deps: &mut BTreeMap<String, String>| -> Vec<f64> {
            if let Some((k, v)) = dep(self.reg, TRANSFER_FORMAT, &t.header.id) {
                deps.insert(k, v);
            }
            stream_cache
                .entry(t.header.id.clone())
                .or_insert_with(|| transferred(t, wl, r_meas, &sctx))
                .clone()
        };
        // no correction for this instrument is installed: uncalibrated (the band calls are off below)
        let uncal = matches!(selection, Selection::Missing);
        let (r_std, skey, stream_t): (Vec<f64>, String, Option<&Transfer>) = match selection {
            Selection::Missing => {
                // no correction for this instrument is installed: every result is uncalibrated for it
                if let Some(V::Map(o)) = rec.get_mut("instrument") {
                    if let Some(V::List(n)) = o.get_mut("notes") {
                        n.push(V::Map(
                            Obj::new()
                                .with("key", "instrument_uncalibrated")
                                .with("class", cls),
                        ));
                    }
                }
                (r_meas.to_vec(), NO_TRANSFER.into(), None)
            }
            Selection::NotNeeded => (r_meas.to_vec(), NO_TRANSFER.into(), None),
            Selection::Apply(t) => (apply(t, &mut deps), t.file_sha256.clone(), Some(t)),
        };
        rec.set(
            "stream",
            Obj::new()
                .with("transfer", stream_t.map_or("none".to_string(), |t| t.key()))
                .with("transfer_sha256", stream_t.map(|t| t.file_sha256.clone()))
                .with("provisional", stream_t.is_some_and(|t| t.provisional)),
        );
        // ---- Step 4: long-wave eligibility, band readability (gains computed for the stream's correction)
        let lwp = self.cp("longwave");
        self.check_dep("longwave", &mut deps);
        let (lw, tail_unreliable, tail_unreliable_c1) =
            checks::longwave(lwp, native_why, &mut n2c, &route);
        if let Some(V::Map(c)) = rec.get_mut("checks") {
            c.set("longwave", lw);
        }
        if let Some((k, v)) = dep(self.reg, BANDS_FORMAT, &self.bands.header.id) {
            deps.insert(k, v);
        }
        if let Some((k, v)) = dep(self.reg, NOISE_GAINS_FORMAT, &self.gains.header.id) {
            deps.insert(k, v);
        }
        // A band whose noise cannot be estimated (a non-linear correction, a kernel or noise window not inside one
        // detector segment, a non-finite N2) has SD +infinity: unreadable unless strong, never readable by default
        let mut sd_e: BTreeMap<String, f64> = BTreeMap::new();
        let mut unknown = Obj::new();
        for b in self.bands.bands.keys() {
            let Some((sd, st)) = self.bands.readability_sd(
                Some(&self.reg.gain_cache),
                b,
                wl,
                joins,
                scorr,
                &mut |lo, hi| n2c.get(lo, hi),
            ) else {
                continue;
            };
            if st != "assessed" {
                unknown.set(b, st);
            }
            sd_e.insert(b.clone(), sd);
        }
        let mut sdo = Obj::new();
        for (b, v) in &sd_e {
            sdo.set(b, fin(*v));
        }
        let mut bn = Obj::new()
            .with("transfer_key", skey.as_str())
            .with("sd_E", sdo);
        if !unknown.0.is_empty() {
            let ids: Vec<V> = unknown.0.iter().map(|(k, _)| V::from(k.as_str())).collect();
            bn.set("noise_unknown", unknown);
            if let Some(V::Map(o)) = rec.get_mut("instrument") {
                if let Some(V::List(n)) = o.get_mut("notes") {
                    n.push(V::Map(
                        Obj::new()
                            .with("key", "band_noise_unknown")
                            .with("bands", V::List(ids)),
                    ));
                }
            }
        }
        rec.set("band_noise", bn);
        // ---- Step 5: spectral consumers on the standard-resolution-equivalent stream
        let evp = self.cp("evidence_levels");
        self.check_dep("evidence_levels", &mut deps);
        let CheckParams::EvidenceLevels { kernel: ek, .. } = evp else {
            return rec;
        };
        let e = kernels::derivative(&r_std, ek);
        // B9 (long-wave policy: truncated variant or not assessed)
        let CheckParams::Longwave {
            b9_mode, c1_mode, ..
        } = lwp
        else {
            return rec;
        };
        self.check_dep("b9", &mut deps);
        let b9r = match self.cp("b9") {
            CheckParams::B9 {
                kernel,
                std_epsilon,
                full,
                truncated,
            } => {
                if tail_unreliable && b9_mode != "truncated" {
                    Obj::new()
                        .with("status", "not assessed: long-wave region too noisy")
                        .with("score", V::Null)
                        .with("outcome", V::Null)
                } else if tail_unreliable {
                    match truncated {
                        Some(t) => checks::b9(wl, &r_std, kernel, *std_epsilon, t, true),
                        None => Obj::new()
                            .with("status", "not assessed: long-wave region too noisy")
                            .with("score", V::Null)
                            .with("outcome", V::Null),
                    }
                } else {
                    checks::b9(wl, &r_std, kernel, *std_epsilon, full, false)
                }
            }
            _ => Obj::new(),
        };
        let notfit = b9r.get("outcome").and_then(V::as_str) == Some("fail");
        if let Some(V::Map(c)) = rec.get_mut("checks") {
            c.set("B9", b9r);
        }
        // models: the union of the profiles' lists; each profile reads its OWN verdict input and second
        // opinion (Codex Phase 3 review 1)
        let (models, per_profile, noise) = self.models(
            wl, r_meas, &sctx, &mut n2c, cls, native, &mut apply, &mut deps,
        );
        let mut mo = models;
        for (k, mr) in mo.0.iter_mut() {
            if let V::Map(mr) = mr {
                let sd = mr.get("implied_sd").and_then(V::as_f64);
                let value = mr.get("value").and_then(V::as_f64);
                let (status, applies) = noise
                    .get(k)
                    .map(|(s, a)| (s.as_str(), *a))
                    .unwrap_or((NO_GAIN, true));
                mr.set(
                    "B6",
                    b6_noise(
                        value,
                        sd,
                        status,
                        applies,
                        b6_cuts,
                        *b6_check_if_cut_within_sd,
                    ),
                );
            }
        }
        rec.set("models", mo);
        // evidence and ZooMS
        let (mut ev, level, s_val) = checks::evidence(wl, &e, evp, self.bands, &sd_e);
        self.check_dep("zooms_patterns", &mut deps);
        let (mut zo, zv_shown, zv, zp) =
            checks::zooms(wl, &e, self.cp("zooms_patterns"), self.bands, &sd_e);
        // an uncalibrated instrument: the band calls are off (DECISIONS 137, amended 2026-10-19)
        if uncal {
            checks::band_evidence_off(&mut ev, &mut zo);
        }
        rec.set("evidence", ev);
        let zo_rec = zo.clone();
        rec.set("zooms_pattern", zo);
        // signs and C1
        self.check_dep("signs", &mut deps);
        self.check_dep("heat", &mut deps);
        self.check_dep("c1", &mut deps);
        // heat reads the corrected stream when the correction's visible levels were fitted (or none applies), else the
        // scan as measured, with a note (DECISIONS 137: the levels are not display-only)
        let fitted = stream_t.is_none_or(|t| t.visible_levels_fitted);
        if !fitted {
            if let Some(V::Map(o)) = rec.get_mut("instrument") {
                if let Some(V::List(n)) = o.get_mut("notes") {
                    n.push(V::Map(Obj::new().with("key", "visible_levels_not_fitted")));
                }
            }
        }
        let mut sg = checks::signs(
            wl,
            &r_std,
            self.cp("signs"),
            self.cp("heat"),
            &gates,
            &reduced,
            (!fitted).then_some(r_meas),
        );
        let c1p = self.cp("c1");
        let spec = match c1p {
            CheckParams::C1 {
                runs_only_if_not_fired,
                ..
            } => runs_only_if_not_fired.iter().any(|n| {
                sg.get(n)
                    .and_then(V::as_obj)
                    .and_then(|o| o.get("fired"))
                    .is_some_and(V::truthy)
            }),
            _ => false,
        };
        let c1r = checks::c1(
            wl,
            &e,
            c1p,
            gates.get("C1").and_then(|g| g.as_deref()),
            spec,
            tail_unreliable_c1,
            c1_mode,
            reduced.get("C1").map(String::as_str),
        );
        let soft = checks::soft_tier(self.cp("signs"), c1p, native, &mut n2c, &sg, &c1r, &route);
        sg.set("soft", soft);
        // ---- Step 9: verdicts
        let mut verdicts = Vec::new();
        let models_rec = rec
            .get("models")
            .and_then(V::as_obj)
            .cloned()
            .unwrap_or_else(Obj::new);
        {
            let x = VerdictInputs {
                signs: &sg,
                c1: &c1r,
                level: &level,
                zooms_verdict: &zv,
                zooms_shown_verdict: &zv_shown,
                zooms_pattern: &zp,
                zooms_rec: &zo_rec,
                models: &models_rec,
                gated,
                uncalibrated: uncal,
            };
            for (ai, a) in ANALYSES.into_iter().enumerate() {
                let p = self.profiles[a];
                let (m, comps, s1) = per_profile[ai].clone();
                // B9 ("Doesn't look like bone") replaces the verdict on every instrument, also one not built in, where
                // the readings otherwise set it (DECISIONS 137): a deliberate exception. B9 is an input screen, not band
                // evidence: the highest Pearson correlation of the scan's D2 profile with bone prototypes, a shape
                // test that a uniform band-depth (calibration) change does not move; it is about rock and other
                // targets, not bone quality (DECISIONS 118). On the 61 real FieldSpec 3 / Trek scans it passed on all.
                let mut v = if notfit {
                    Obj::new()
                        .with("verdict", "Doesn't look like bone")
                        .with("rule_step", "B9")
                        .with("notes_all", V::List(vec![]))
                        .with("notes_shown", V::List(vec![]))
                        .with("flags", verdict::flags_for(p, &x))
                } else {
                    verdict::verdict_for(p, m, &comps, &x)
                };
                if !notfit {
                    if let (Some(so), Some(s1), Some(mm)) = (&p.second_opinion, s1, m) {
                        // the correction's own gap when it carries one (from >= 20 scans), else the profile's default
                        let gap = stream_t
                            .and_then(|t| t.second_opinion_gap)
                            .unwrap_or(so.gap_points);
                        // S1_R2 stays shown on an uncalibrated instrument, but its second-opinion note is not
                        // added: the instrument note already says the readings are uncalibrated (DECISIONS 137)
                        if !uncal && so.streams.applies(native, cls) && (s1 - mm).abs() > gap {
                            if let Some(V::List(n)) = v.get_mut("notes_all") {
                                n.push(V::Map(
                                    Obj::new()
                                        .with("key", "second_opinion_differs")
                                        .with("s1", s1)
                                        .with("m", mm),
                                ));
                            }
                        }
                    }
                }
                let vd = v
                    .get("verdict")
                    .and_then(V::as_str)
                    .unwrap_or("")
                    .to_string();
                let (g, val) = verdict::sort_key(p, &vd, m, s_val);
                v.set("sort_group", g);
                v.set("sort_value", val);
                if let Some(pen) = verdict::sort_penalty(p, &v) {
                    v.set("sort_penalty", pen);
                }
                verdicts.push((a, v));
            }
        }
        rec.set("signs", sg);
        rec.set("C1", c1r);
        for (a, v) in verdicts {
            rec.set(a, v);
        }
        rec.set("n2", n2c.table());
        rec.set("dependencies", Self::dependencies(deps));
        rec
    }

    /// The profiles' models (union, first profile first): transfer per model (Step 4 rule; a model whose spec reads
    /// `as_measured` runs on the scan as measured for any class), prediction (plus the correction's reader offset, a
    /// consensus's on its components before the median), noise SD (computed for the model and the transfer applied),
    /// domain note. Returns (record, per profile in ANALYSES order: (m = its verdict input, that model's components,
    /// its second opinion), per model key: (noise status, whether B6 applies)).
    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    fn models(
        &self,
        wl: &[f64],
        r_meas: &[f64],
        sctx: &ScanContext,
        n2c: &mut N2Cache,
        cls: &str,
        native: bool,
        apply: &mut impl FnMut(&Transfer, &mut BTreeMap<String, String>) -> Vec<f64>,
        deps: &mut BTreeMap<String, String>,
    ) -> (Obj, Vec<PerProfile>, BTreeMap<String, (String, bool)>) {
        let mut noise: BTreeMap<String, (String, bool)> = BTreeMap::new();
        let pps: Vec<&AnalysisProfile> = ANALYSES.iter().map(|a| self.profiles[*a]).collect();
        let transfers = self.reg.transfers();
        let pinned = self.reg.pinned_transfer_ids();
        let mut out = Obj::new();
        let mut specs: Vec<&crate::plugins::profiles::ProfileModel> = Vec::new();
        for pp in &pps {
            for s in &pp.models {
                if !specs.iter().any(|x| x.key == s.key) {
                    specs.push(s);
                }
            }
        }
        let dn = pps[0].domain_note_ratio_above;
        for spec in specs {
            let shown = pps.iter().any(|pp| {
                pp.models.iter().any(|x| {
                    x.key == spec.key
                        && (x.shown_for.applies(native, cls) || x.role == "verdict_input")
                })
            });
            if !shown {
                continue;
            }
            let Some(md) = self.reg.model(&spec.model_id) else {
                out.set(
                    &spec.key,
                    Obj::new()
                        .with("id", spec.model_id.as_str())
                        .with("status", "not assessed: model not loaded")
                        .with("value", V::Null),
                );
                continue;
            };
            let entry = self.reg.selected_entry(MODEL_FORMAT, &md.header.id);
            if let Some(e) = entry {
                deps.insert(e.label(), e.sha256.clone());
            }
            let sel = crate::transfer::select_for_reader(
                spec.reads_as_measured,
                md,
                sctx,
                &transfers,
                &pinned,
            );
            let (x, corr, tref, tsel) = match sel {
                Selection::Missing => (
                    r_meas.to_vec(),
                    None,
                    "none (no transfer available)".to_string(),
                    None,
                ),
                Selection::NotNeeded => (r_meas.to_vec(), None, "none".to_string(), None),
                Selection::Apply(t) => (apply(t, deps), Some(t.correction()), t.key(), Some(t)),
            };
            let (mut pred, status) = match md.predict(&Spectrum::new(wl.to_vec(), x), sctx) {
                Ok(p) => (Some(p), "assessed".to_string()),
                Err(e) => (None, format!("not assessed: {e}")),
            };
            // the correction's per-reader offsets (DECISIONS 137): on a consensus, its components before the median
            let mut reader_offset = None;
            let mut component_offsets = Obj::new();
            if let (Some(t), Some(p)) = (tsel, pred.as_mut()) {
                if !t.reader_offsets.is_empty() {
                    let (own, comps) = md.add_reader_offsets(p, &t.reader_offsets);
                    reader_offset = own;
                    for (c, off) in comps {
                        component_offsets.set(&c, off);
                    }
                }
            }
            let value = pred.as_ref().map(|p| p.value).filter(|v| v.is_finite());
            let dratio = pred
                .as_ref()
                .and_then(|p| p.domain_ratio)
                .filter(|v| v.is_finite());
            let mut r = Obj::new()
                .with("id", md.key())
                .with("sha256", entry.map(|e| e.sha256.clone()))
                .with("role", spec.role.as_str())
                .with("transfer", tref)
                .with("status", status)
                .with("value", value)
                .with("domain_ratio", dratio);
            let isd = self.gains.implied_sd(
                Some(&self.reg.gain_cache),
                md,
                wl,
                &sctx.splices_nm,
                corr,
                &mut |lo, hi| n2c.get(lo, hi),
            );
            if spec.role == "verdict_input" {
                let feats: BTreeMap<String, f64> = pred
                    .as_ref()
                    .map(|p| p.components.iter().cloned().collect())
                    .unwrap_or_default();
                let mut co = Obj::new();
                for c in &spec.components {
                    co.set(c, feats.get(c).copied().filter(|v| v.is_finite()));
                }
                r.set("components", co);
                if let Some(cs) = &isd.component_sd {
                    let mut csd = Obj::new();
                    for (c, sd) in cs {
                        csd.set(c, *sd);
                    }
                    r.set("component_sd", csd);
                }
            }
            r.set("implied_sd", isd.sd);
            if let Some(off) = reader_offset {
                r.set("reader_offset", off);
            }
            if !component_offsets.0.is_empty() {
                r.set("component_offsets", component_offsets);
            }
            noise.insert(spec.key.clone(), (isd.status, isd.b6));
            r.set(
                "domain_note",
                matches!((dn, dratio), (Some(d), Some(x)) if x > d),
            );
            out.set(&spec.key, r);
        }
        let per = pps
            .iter()
            .map(|pp| {
                let (mut m, mut comps, mut s1) = (None, Vec::new(), None);
                for x in &pp.models {
                    let Some(r) = out.get(&x.key).and_then(V::as_obj) else {
                        continue;
                    };
                    let value = r.get("value").and_then(V::as_f64);
                    if x.role == "verdict_input" {
                        m = value;
                        comps = r
                            .get("components")
                            .and_then(V::as_obj)
                            .map(|c| c.0.iter().map(|(k, v)| (k.clone(), v.as_f64())).collect())
                            .unwrap_or_default();
                    } else if x.role == "second_opinion" {
                        s1 = value;
                    }
                }
                (m, comps, s1)
            })
            .collect();
        (out, per, noise)
    }
}

/// The transferred stream on the scan's grid. A transfer that fails, or whose output is not on the scan's grid (a
/// block-SNV correction keeps only its block's points), gives a NaN stream: its consumers are not assessed, never
/// read off the wrong channels.
fn transferred(t: &Transfer, wl: &[f64], r: &[f64], ctx: &ScanContext) -> Vec<f64> {
    match t.apply(&Spectrum::new(wl.to_vec(), r.to_vec()), ctx) {
        Ok(s) if s.wl == wl => s.x,
        _ => vec![f64::NAN; wl.len()],
    }
}

/// One profile's model readings: (verdict input m, its components, the second opinion).
type PerProfile = (Option<f64>, Vec<(String, Option<f64>)>, Option<f64>);

/// A verdict block with no notes or flags (rejected, unusable).
fn empty_verdict(verdict: &str, step: &str) -> Obj {
    Obj::new()
        .with("verdict", verdict)
        .with("rule_step", step)
        .with("notes_all", V::List(vec![]))
        .with("notes_shown", V::List(vec![]))
        .with("flags", V::List(vec![]))
}

/// B6 status of a model with no noise record at all (not loaded, not computed).
const NO_GAIN: &str = "not assessed: no noise gain for this model and transfer";

/// B6 (DECISIONS 109; oracle `pipeline.b6_noise`): Check when a verdict cut lies within `within_sd` x the implied SD
/// of the reading, i.e. when noise alone could carry the reading across a cut. Not applied to an exempt reader; no
/// SD: not assessed, with the noise status (a non-linear correction, not available on this instrument, ...); no
/// finite reading: no outcome.
fn b6_noise(
    value: Option<f64>,
    sd: Option<f64>,
    status: &str,
    applies: bool,
    cuts: &[f64],
    within_sd: f64,
) -> Obj {
    if !applies {
        return Obj::new()
            .with("status", B6_NOT_APPLIED)
            .with("outcome", None::<&str>);
    }
    let Some(sd) = sd.filter(|s| s.is_finite()) else {
        return Obj::new()
            .with(
                "status",
                if status == "assessed" {
                    NO_GAIN
                } else {
                    status
                },
            )
            .with("outcome", None::<&str>);
    };
    let outcome = value.filter(|v| v.is_finite()).map(|v| {
        if cuts.iter().any(|c| (v - c).abs() <= within_sd * sd) {
            "Check"
        } else {
            "ok"
        }
    });
    Obj::new()
        .with("status", "assessed")
        .with("outcome", outcome)
}

// ---------------------------------------------------------------------- .asd metadata (oracle asd.read)
const OLE_EPOCH_UNIX_DAYS: i64 = 25569;

/// CPython `timedelta(days=d)` as (whole days, seconds, microseconds), d >= 0.
fn timedelta_days(d: f64) -> (i64, i64, i64) {
    let days = d.trunc();
    let dayfrac = d - days;
    let secs = dayfrac * 86400.0;
    let whole = secs.trunc();
    let frac = secs - whole;
    let mut us = (frac * 1e6).round_ties_even() as i64;
    let mut s = whole as i64;
    let mut dd = days as i64;
    if us >= 1_000_000 {
        us -= 1_000_000;
        s += 1;
    }
    if s >= 86400 {
        s -= 86400;
        dd += 1;
    }
    (dd, s, us)
}

/// The oracle's `ole_to_iso`: 1899-12-30 + timedelta(days=ole), ISO to the second (microseconds truncated).
pub fn ole_to_iso(ole: Option<f64>) -> Option<String> {
    let ole = ole.filter(|x| x.is_finite() && *x >= 0.0 && *x < 1e7)?;
    let (d, s, _) = timedelta_days(ole);
    let (y, mo, da) = crate::timefmt::civil_from_days(d - OLE_EPOCH_UNIX_DAYS);
    Some(format!(
        "{y:04}-{mo:02}-{da:02}T{:02}:{:02}:{:02}",
        s / 3600,
        (s / 60) % 60,
        s % 60
    ))
}

/// The oracle's `utc_offset_minutes`: local (OLE reference time) minus UTC (time_t), rounded to 15 minutes.
pub fn utc_offset_minutes(ole_ref: Option<f64>, ref_time_t: i32) -> Option<i64> {
    let ole = ole_ref.filter(|x| x.is_finite() && *x >= 0.0 && *x < 1e7)?;
    if ref_time_t == 0 {
        return None;
    }
    let (d, s, us) = timedelta_days(ole);
    let local_us = ((d - OLE_EPOCH_UNIX_DAYS) * 86400 + s) as i128 * 1_000_000 + us as i128;
    let diff_us = local_us - i128::from(ref_time_t) * 1_000_000;
    let m = (diff_us as f64 / 1e6) / 60.0;
    Some(((m / 15.0).round_ties_even() * 15.0) as i64)
}

/// The record's `input` block for an `.asd` file (oracle `asd.read` header fields). Err = the oracle's reader
/// error (reason, meta so far).
fn asd_meta(b: &[u8], name: &str) -> Result<Obj, (String, Obj)> {
    let base = Obj::new().with("file", name);
    if b.len() < 484 {
        return Err(("unsupported: file shorter than the header".into(), base));
    }
    let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
    let u32_at = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let i32_at = |o: usize| i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    if !matches!(b[199], 0..=2) {
        return Err(("unsupported: unknown data format".into(), base));
    }
    // the oracle reader, tolerant as it is: format, OLE dates and the resolved joins (DECISIONS 87)
    let Some(pr) = crate::read::asd::probe(b) else {
        return Err(("unsupported: truncated sample block".into(), base));
    };
    let mut m = base
        .with("input_sha256", sha256_hex(b))
        .with("input_kind", "asd")
        .with("serial", i64::from(u16_at(400)))
        .with("time_ole", pr.ole_spectrum)
        .with("time_local", ole_to_iso(pr.ole_spectrum))
        .with(
            "utc_offset_min",
            utc_offset_minutes(pr.ole_reference, i32_at(187)),
        )
        .with("integration_time_ms", i64::from(u32_at(390)))
        .with("averages", i64::from(u16_at(429)))
        .with(
            "splices_nm",
            V::List(pr.joins.iter().map(|x| V::Num(*x)).collect()),
        );
    // other instruments: recorded only when they differ from the LabSpec as8 case, so as8 records are unchanged
    if pr.format != Some("as8") {
        m.set("format", pr.format.map(V::from).unwrap_or(V::Null));
    }
    if pr.joins_source != "header" {
        m.set("joins_source", pr.joins_source);
    }
    Ok(m)
}

/// Each join inside its window, in sorted order (oracle `asd.joins_accepted`).
fn joins_accepted(joins: &[f64], windows: &[(f64, f64)]) -> bool {
    let mut j = joins.to_vec();
    j.sort_by(f64::total_cmp);
    j.len() == windows.len()
        && j.iter()
            .zip(windows)
            .all(|(x, (lo, hi))| lo <= x && x <= hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DECISIONS 109; expected values written from the rule (LESSONS 44), as reference/tests/test_b6_cut_aware.py.
    #[test]
    fn b6_is_cut_aware() {
        let cuts = [0.5, 3.0];
        let out = |v: Option<f64>, sd: Option<f64>| {
            b6_noise(v, sd, "assessed", true, &cuts, 2.6)
                .get("outcome")
                .and_then(V::as_str)
                .map(str::to_string)
        };
        let check = Some("Check".to_string());
        let ok = Some("ok".to_string());
        assert_eq!(out(Some(3.4), Some(0.2)), check); // 0.4 <= 0.52
        assert_eq!(out(Some(0.1), Some(0.16)), check); // 0.4 <= 0.416
        assert_eq!(out(Some(8.0), Some(1.5)), ok); // 5 > 3.9 (the old SD > 0.5 rule fired)
        assert_eq!(out(Some(1.7), Some(0.4)), ok); // 1.2 and 1.3 > 1.04
        assert_eq!(out(Some(12.0), Some(4.0)), check); // 9 <= 10.4
        assert_eq!(out(None, Some(0.3)), None);
        assert_eq!(out(Some(f64::NAN), Some(0.3)), None);
        let na = b6_noise(Some(2.0), None, NO_GAIN, true, &cuts, 2.6);
        assert_eq!(
            na.get("status").and_then(V::as_str),
            Some("not assessed: no noise gain for this model and transfer")
        );
        assert_eq!(na.get("outcome").and_then(V::as_str), None);
        // the noise status says why: a non-linear correction; a reader B6 does not apply to
        let nl = b6_noise(
            Some(2.0),
            None,
            "not assessed: non-linear correction",
            true,
            &cuts,
            2.6,
        );
        assert_eq!(
            nl.get("status").and_then(V::as_str),
            Some("not assessed: non-linear correction")
        );
        let ex = b6_noise(Some(3.1), Some(0.5), "assessed", false, &cuts, 2.6);
        assert_eq!(ex.get("status").and_then(V::as_str), Some(B6_NOT_APPLIED));
        assert_eq!(ex.get("outcome").and_then(V::as_str), None);
        // a non-finite SD is never "ok"
        let inf = b6_noise(Some(8.0), Some(f64::NAN), "assessed", true, &cuts, 2.6);
        assert_eq!(inf.get("outcome").and_then(V::as_str), None);
    }
}
