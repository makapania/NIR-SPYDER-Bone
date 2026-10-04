//! Display arrays for the charts (PLAN Step 10), computed in Rust and sent to the UI as float32 over the binary
//! path. The UI computes nothing numerical; it only draws these.
//!
//! Every scan gets the same eight views on the 1 nm grid (`VIEW_ORDER`):
//! * `R`, `A`, `D2`: reflectance, absorbance and -d2A/dl2 at the chart smoothing, on the
//!   standard-resolution-equivalent stream (the transferred spectrum on high-res; the scan itself on
//!   standard-res). This is what every spectral consumer reads.
//! * `D2_31`: the evidence kernel itself (`e31`: -1e5 x SG 31 cubic 2nd derivative of log10(1/max(R, 1e-6)))
//!   on that stream: the curve the band rule, the ZooMS pattern and the model windows read. The band close-ups
//!   and the "As measured" model-window close-ups draw it, so their shading matches the calls exactly.
//! * `D2_31_ohc`: the OH-corrected model windows (2030-2060 and 1500-1550 nm; NaN elsewhere): the window
//!   features with the altered OH / bound-water direction projected out, exactly as the OH-corrected model
//!   sees them (see [`OhWindow`]).
//! * `R_meas`, `A_meas`, `D2_meas`: the scan as measured (identical to the first three on standard-res).
//!
//! The chart smoothing changes only `D2` and `D2_meas`; the models and the band rule keep SG 31.

use serde::Serialize;
use spyder_core::pipeline::kernels;
use spyder_core::pipeline::Engine;
use spyder_core::plugins::checks::CheckParams;
use spyder_core::plugins::registry::Registry;
use spyder_core::plugins::tables::Kernel;
use spyder_core::plugins::{ScanContext, CLASS_STD, MODEL_FORMAT};
use spyder_core::preprocess::{absorbance, Spectrum};

/// The order of the float32 views in one binary block (n values each).
pub const VIEW_ORDER: [&str; 8] = [
    "R",
    "A",
    "D2",
    "D2_31",
    "D2_31_ohc",
    "R_meas",
    "A_meas",
    "D2_meas",
];

/// The OH-direction file shipped with the app (public-derived; see its `derivation` field).
const OH_DIRECTIONS: &str = include_str!("../resources/oh_directions_v1.json");

/// Orthogonality the projection needs: |b.v| / (|b| |v|) below this, so the corrected view reads exactly as
/// the model does (the folded coefficients are (I - v v^T) b_pls).
const ORTHO_TOL: f64 = 1e-9;

/// Chart smoothing range (odd 11-51).
pub fn clamp_smoothing(w: u32) -> usize {
    let w = w.clamp(11, 51) as usize;
    if w.is_multiple_of(2) {
        w + 1
    } else {
        w
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OhWindowInfo {
    /// "2045" or "1500".
    pub label: String,
    pub model_id: String,
    pub lo_nm: f64,
    pub hi_nm: f64,
    pub available: bool,
    /// Why the OH-corrected view is not available (plain words), when it is not.
    pub reason: Option<String>,
}

/// One OH-corrected model window. The model reads y = offset + b.(x - c) on its window features x (SG 31
/// second derivative of absorbance, on the stream). Its coefficients are folded, b = (I - v v^T) b_pls, so b is
/// orthogonal to the OH/water direction v. The corrected view is x_ohc = x - v (v.(x - c)) = c + (I - v v^T)
/// (x - c): the scan's departure from the training mean with its OH/water component removed. Because b.v = 0,
/// offset + b.(x_ohc - c) equals the model's reading exactly, so the close-up can never imply a different
/// reading. The references are projected the same way.
#[derive(Clone, Debug)]
pub struct OhWindow {
    pub info: OhWindowInfo,
    wl: Vec<f64>,
    centre: Vec<f64>,
    coef: Vec<f64>,
    offset: f64,
    v: Vec<f64>,
}

impl OhWindow {
    /// The corrected features and the reading they imply (offset + b.(x_ohc - c)), from the raw features.
    pub fn project(&self, x: &[f64]) -> (Vec<f64>, f64) {
        let t: f64 = x
            .iter()
            .zip(&self.centre)
            .zip(&self.v)
            .map(|((xi, ci), vi)| (xi - ci) * vi)
            .sum();
        let xo: Vec<f64> = x.iter().zip(&self.v).map(|(xi, vi)| xi - vi * t).collect();
        let implied = self.offset
            + xo.iter()
                .zip(&self.centre)
                .zip(&self.coef)
                .map(|((xi, ci), bi)| (xi - ci) * bi)
                .sum::<f64>();
        (xo, implied)
    }

    /// The model's own reading from the raw features (what `predict` returns for this regression).
    pub fn direct(&self, x: &[f64]) -> f64 {
        self.offset
            + x.iter()
                .zip(&self.centre)
                .zip(&self.coef)
                .map(|((xi, ci), bi)| (xi - ci) * bi)
                .sum::<f64>()
    }

    pub fn wavelengths(&self) -> &[f64] {
        &self.wl
    }

    pub fn direction(&self) -> &[f64] {
        &self.v
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub view_order: Vec<&'static str>,
    pub start_nm: f64,
    pub step_nm: f64,
    pub n: usize,
    pub oh_windows: Vec<OhWindowInfo>,
    /// Plain-words reason when no chart can be drawn (the startup-error state).
    pub unavailable: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefMeta {
    pub id: String,
    pub label: String,
    pub legend: String,
    pub mean_yield_pct: f64,
    pub n: usize,
}

pub struct DisplayKit {
    kernel: Option<Kernel>,
    windows: Vec<OhWindow>,
    unavailable: Option<String>,
}

/// A spectrum to draw: the as-measured reflectance with its joins and serial.
pub struct DrawInput<'a> {
    pub wl: &'a [f64],
    pub r: &'a [f64],
    pub joins: &'a [f64],
    pub serial: Option<u64>,
}

fn evidence_kernel(reg: &Registry) -> Option<Kernel> {
    reg.checks()
        .into_iter()
        .find(|c| c.check == "evidence_levels")
        .and_then(|c| match &c.params {
            CheckParams::EvidenceLevels { kernel, .. } => Some(kernel.clone()),
            _ => None,
        })
}

fn f64_list(v: &serde_json::Value) -> Option<Vec<f64>> {
    v.as_array()?
        .iter()
        .map(serde_json::Value::as_f64)
        .collect()
}

fn load_window(reg: &Registry, d: &serde_json::Value) -> OhWindow {
    let label = d["label"].as_str().unwrap_or("?").to_string();
    let model_id = d["model_id"].as_str().unwrap_or("").to_string();
    let (lo, hi) = match f64_list(&d["window_nm"]).as_deref() {
        Some([a, b]) => (*a, *b),
        _ => (f64::NAN, f64::NAN),
    };
    let mut w = OhWindow {
        info: OhWindowInfo {
            label,
            model_id: model_id.clone(),
            lo_nm: lo,
            hi_nm: hi,
            available: false,
            reason: None,
        },
        wl: f64_list(&d["wavelengths_nm"]).unwrap_or_default(),
        centre: Vec::new(),
        coef: Vec::new(),
        offset: f64::NAN,
        v: f64_list(&d["direction"]).unwrap_or_default(),
    };
    let fail = |mut w: OhWindow, why: &str| {
        w.info.reason = Some(why.to_string());
        w
    };
    let Some(model) = reg.model(&model_id) else {
        return fail(w, "the OH-corrected model is not loaded");
    };
    let entry = reg.selected_entry(MODEL_FORMAT, &model_id);
    let same_file = entry.is_some_and(|e| Some(e.sha256.as_str()) == d["model_sha256"].as_str());
    if model.header.version.to_string() != d["model_version"].as_str().unwrap_or("") || !same_file {
        return fail(
            w,
            "the OH/water direction shipped with the app belongs to another version of this model",
        );
    }
    let spyder_core::model::Body::Regression {
        coefficients,
        x_center: Some(c),
        offset,
    } = &model.body
    else {
        return fail(w, "the model is not a centred linear regression");
    };
    if model.features_nm != w.wl || w.v.len() != w.wl.len() || coefficients.len() != w.wl.len() {
        return fail(w, "the direction does not match the model's window");
    }
    let nv = w.v.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb = coefficients.iter().map(|x| x * x).sum::<f64>().sqrt();
    if !(nv > 0.0 && nb > 0.0) {
        return fail(w, "the direction is empty");
    }
    for x in &mut w.v {
        *x /= nv;
    }
    let bv: f64 = coefficients.iter().zip(&w.v).map(|(b, v)| b * v).sum();
    if (bv / nb).abs() > ORTHO_TOL {
        return fail(
            w,
            "the model's coefficients are not OH-corrected along this direction",
        );
    }
    w.centre = c.clone();
    w.coef = coefficients.clone();
    w.offset = *offset;
    w.info.available = true;
    w
}

fn grid(n: usize) -> Vec<f64> {
    (0..n).map(|i| 350.0 + i as f64).collect()
}

impl DisplayKit {
    pub fn new(reg: &Registry) -> DisplayKit {
        let kernel = evidence_kernel(reg);
        let doc: serde_json::Value = serde_json::from_str(OH_DIRECTIONS).unwrap_or_default();
        let windows = doc["directions"]
            .as_array()
            .map(|a| a.iter().map(|d| load_window(reg, d)).collect())
            .unwrap_or_default();
        DisplayKit {
            unavailable: kernel
                .is_none()
                .then(|| "no evidence kernel is loaded".to_string()),
            kernel,
            windows,
        }
    }

    pub fn unavailable(reason: &str) -> DisplayKit {
        DisplayKit {
            kernel: None,
            windows: Vec::new(),
            unavailable: Some(reason.to_string()),
        }
    }

    pub fn info(&self) -> DisplayInfo {
        DisplayInfo {
            view_order: VIEW_ORDER.to_vec(),
            start_nm: 350.0,
            step_nm: 1.0,
            n: 2151,
            oh_windows: self.windows.iter().map(|w| w.info.clone()).collect(),
            unavailable: self.unavailable.clone(),
        }
    }

    pub fn window(&self, label: &str) -> Option<&OhWindow> {
        self.windows
            .iter()
            .find(|w| w.info.label == label && w.info.available)
    }

    fn d2(&self, r: &[f64], window: usize) -> Vec<f64> {
        match &self.kernel {
            Some(k) => {
                let mut k = k.clone();
                k.savgol.window = window;
                kernels::derivative(r, &k)
            }
            None => vec![f64::NAN; r.len()],
        }
    }

    fn absorbance(&self, r: &[f64]) -> Vec<f64> {
        let clip = self.kernel.as_ref().map_or(1e-6, |k| k.absorbance_clip_min);
        absorbance(r, clip)
    }

    fn scale(&self) -> f64 {
        self.kernel.as_ref().map_or(-1e5, |k| k.scale)
    }

    /// The OH-corrected windows on a stream: NaN outside the windows. A window whose implied reading differs
    /// from the model's own reading (it cannot, by construction; this is a guard) stays NaN.
    fn ohc(&self, reg: &Registry, wl: &[f64], stream: &[f64], joins: &[f64]) -> Vec<f64> {
        let mut out = vec![f64::NAN; wl.len()];
        let scale = self.scale();
        let ctx = ScanContext::new(joins.to_vec(), CLASS_STD, None);
        for w in self.windows.iter().filter(|w| w.info.available) {
            let Some(model) = reg.model(&w.info.model_id) else {
                continue;
            };
            let Ok(x) = model.features(&Spectrum::new(wl.to_vec(), stream.to_vec()), &ctx) else {
                continue;
            };
            let (xo, implied) = w.project(&x);
            let direct = w.direct(&x);
            if !(implied - direct).abs().le(&(1e-9 * direct.abs().max(1.0))) {
                continue;
            }
            for (lam, v) in w.wl.iter().zip(&xo) {
                if let Some(i) = spyder_core::grid::index_of(wl, *lam) {
                    out[i] = scale * v;
                }
            }
        }
        out
    }

    /// The eight views of one scan (VIEW_ORDER), n values each, float64 (narrowed to float32 at the boundary).
    pub fn scan_views(
        &self,
        reg: &Registry,
        eng: &Engine,
        input: &DrawInput,
        class: &str,
        smoothing: usize,
    ) -> Vec<f64> {
        let (stream, _) = eng.standard_stream(input.wl, input.r, input.joins, class, input.serial);
        let mut out = Vec::with_capacity(input.wl.len() * VIEW_ORDER.len());
        out.extend_from_slice(&stream);
        out.extend(self.absorbance(&stream));
        out.extend(self.d2(&stream, smoothing));
        out.extend(self.d2(&stream, 31));
        out.extend(self.ohc(reg, input.wl, &stream, input.joins));
        out.extend_from_slice(input.r);
        out.extend(self.absorbance(input.r));
        out.extend(self.d2(input.r, smoothing));
        out
    }

    /// The reference spectra (about 0, 1, 3, 6 and 10% collagen): meta and their eight views each (the
    /// as-measured views repeat the first three: the references are standard-resolution means).
    pub fn reference_views(&self, reg: &Registry, smoothing: usize) -> (Vec<RefMeta>, Vec<f64>) {
        let Some(rs) = reg.reference_sets().into_iter().next() else {
            return (Vec::new(), Vec::new());
        };
        let wl = grid(rs.grid.2);
        let joins = [1000.0, 1800.0];
        let mut meta = Vec::new();
        let mut out = Vec::new();
        for l in &rs.levels {
            let r: Vec<f64> = l.absorbance.iter().map(|a| 10f64.powf(-a)).collect();
            let d = self.d2(&r, smoothing);
            out.extend_from_slice(&r);
            out.extend_from_slice(&l.absorbance);
            out.extend_from_slice(&d);
            out.extend(self.d2(&r, 31));
            out.extend(self.ohc(reg, &wl, &r, &joins));
            out.extend_from_slice(&r);
            out.extend_from_slice(&l.absorbance);
            out.extend_from_slice(&d);
            meta.push(RefMeta {
                id: format!("ref-{}", l.label),
                label: l.label.clone(),
                legend: l.display_label.clone(),
                mean_yield_pct: l
                    .label
                    .trim_end_matches('%')
                    .trim()
                    .parse()
                    .unwrap_or(f64::NAN),
                n: l.n,
            });
        }
        (meta, out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoothing_is_odd_and_in_range() {
        assert_eq!(clamp_smoothing(31), 31);
        assert_eq!(clamp_smoothing(30), 31);
        assert_eq!(clamp_smoothing(3), 11);
        assert_eq!(clamp_smoothing(99), 51);
    }

    #[test]
    fn projection_keeps_the_reading_and_removes_the_direction() {
        // b orthogonal to v; any x
        let v = vec![0.6, 0.8, 0.0];
        let w = OhWindow {
            info: OhWindowInfo {
                label: "t".into(),
                model_id: "m".into(),
                lo_nm: 0.0,
                hi_nm: 2.0,
                available: true,
                reason: None,
            },
            wl: vec![0.0, 1.0, 2.0],
            centre: vec![0.1, -0.2, 0.3],
            coef: vec![4.0, -3.0, 2.0],
            offset: 1.5,
            v: v.clone(),
        };
        let x = [0.7, 0.4, -1.1];
        let (xo, implied) = w.project(&x);
        assert!((implied - w.direct(&x)).abs() < 1e-12);
        let along: f64 = xo
            .iter()
            .zip(&w.centre)
            .zip(&v)
            .map(|((a, c), vi)| (a - c) * vi)
            .sum();
        assert!(along.abs() < 1e-12);
    }
}
