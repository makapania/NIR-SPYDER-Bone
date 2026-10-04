//! Engine-check parameter files (`spyder-bone/engine_check`, PLAN section 4): built-in checks whose parameters
//! live in data files (no rule expression language in v1). Phase 2 loads and validates them against
//! `plugins/schemas/engine_check.<check>.schema.json` (required fields, types, ranges, sidecars) and checks that
//! their golden blocks can test something (>= 3 cases, non-empty `expected`, tolerance <= 1e-9). Evaluating the
//! checks and running those goldens (spec: `reference/oracle/checkgold.py`) is Phase 3.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;

use super::golden::check_deferred_block;
use super::json::load_sidecar;
use super::npy::Array;
use super::tables::{parse_sg, Kernel};
use super::{parse_header, ErrorKind, Header, Node, PResult, PluginError, ENGINE_CHECK_FORMAT};

/// `sharp(c, h, f0, f1)` = mean N[c-h..c+h] - 0.5 (mean N[c-f1..c-f0] + mean N[c+f0..c+f1]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sharp {
    /// The centre as Python's str() of the file's number ("1766"): the key of wax statistics in exports.
    pub centre_key: String,
    pub centre_nm: f64,
    pub half_width: u64,
    pub flank_from: u64,
    pub flank_to: u64,
}

fn sharp(n: &Node) -> PResult<Sharp> {
    let v = n.vec_f64_len(4)?;
    let int = |x: f64| -> PResult<u64> {
        if x >= 0.0 && x.fract() == 0.0 {
            Ok(x as u64)
        } else {
            Err(n.error("sharp [c, h, f0, f1]: h, f0, f1 must be non-negative integers"))
        }
    };
    let s = Sharp {
        centre_key: crate::pyfmt::json_number_str(&n.v[0]),
        centre_nm: v[0],
        half_width: int(v[1])?,
        flank_from: int(v[2])?,
        flank_to: int(v[3])?,
    };
    if !(s.half_width < s.flank_from && s.flank_from <= s.flank_to) {
        return Err(n.error("sharp [c, h, f0, f1] needs h < f0 <= f1"));
    }
    Ok(s)
}

/// A check kernel: E = scale x SG(log10(1/max(R, absorbance_clip))).
fn kernel(n: &Node) -> PResult<Kernel> {
    let clip = n.req("absorbance_clip")?.f64()?;
    if !(clip > 0.0) {
        return Err(n.error("absorbance_clip must be > 0"));
    }
    let (savgol, mode_stated) = parse_sg(&n.req("savgol")?)?;
    Ok(Kernel {
        absorbance_clip_min: clip,
        savgol,
        mode_stated,
        scale: n.req("scale")?.f64()?,
    })
}

fn ranges(n: &Node) -> PResult<Vec<(f64, f64)>> {
    n.arr()?.iter().map(|r| r.range(false)).collect()
}

fn band_names(n: &Node) -> PResult<Vec<String>> {
    let v = n.vec_str()?;
    for b in &v {
        band_name_ok(n, b)?;
    }
    Ok(v)
}

/// A band id: two capitals, four digits, optionally one lower-case suffix (CH1728, NH1545c).
fn band_name_ok(n: &Node, b: &str) -> PResult<()> {
    let ok = b.is_ascii()
        && (b.len() == 6 || b.len() == 7)
        && b[..2].bytes().all(|c| c.is_ascii_uppercase())
        && b[2..6].bytes().all(|c| c.is_ascii_digit())
        && b[6..].bytes().all(|c| c.is_ascii_lowercase());
    if ok {
        Ok(())
    } else {
        Err(n.error(format!("band name {b:?} must look like CH1728 or NH1545c")))
    }
}

/// One sign's B6b gate: the sign is gated when N2(window) on the as-measured scan exceeds the cut.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SignGate {
    pub n2_window_nm: (f64, f64),
    pub n2_above: f64,
}

/// The per-sign B6b gates (phase 0c), keyed by the stream's transfer key.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct B6bGate {
    /// The window of the summary N2 in the record.
    pub n2_window_nm: (f64, f64),
    /// Every gated sign, in order (plaster, wax, ester, burnt, C1).
    pub signs: Vec<String>,
    /// The contaminant signs (the summary's `gated` = all of these gated).
    pub contaminant_signs: Vec<String>,
    /// transfer key ("none" or a transfer file sha256) -> sign -> gate (a sign absent here is never gated).
    pub by_transfer_key: BTreeMap<String, BTreeMap<String, SignGate>>,
    /// The table used for a transfer key without an entry.
    pub unknown_transfer_key_uses: String,
}

impl B6bGate {
    /// The gate table for a transfer key (a key without an entry uses `unknown_transfer_key_uses`).
    pub fn table(&self, key: &str) -> &BTreeMap<String, SignGate> {
        self.by_transfer_key
            .get(key)
            .unwrap_or_else(|| &self.by_transfer_key[&self.unknown_transfer_key_uses])
    }
}

fn b6b_gate(n: &Node) -> PResult<B6bGate> {
    const SIGNS: [&str; 5] = ["plaster", "wax", "ester", "burnt", "C1"];
    let signs = n.req("signs")?.vec_str()?;
    let contaminant_signs = n.req("contaminant_signs")?.vec_str()?;
    for x in signs.iter().chain(&contaminant_signs) {
        if !SIGNS.contains(&x.as_str()) {
            return Err(n.error(format!("unknown sign {x:?}")));
        }
    }
    if contaminant_signs.iter().any(|c| !signs.contains(c)) || signs.is_empty() {
        return Err(n.error("contaminant_signs must be gated signs"));
    }
    let mut by_transfer_key = BTreeMap::new();
    let bt = n.req("by_transfer_key")?;
    for (k, t) in bt.entries()? {
        if !(k == "none" || super::is_sha256_hex(&k)) {
            return Err(bt.error(format!(
                "transfer key {k:?} must be \"none\" or a file sha256"
            )));
        }
        let mut tab = BTreeMap::new();
        for s in &signs {
            if t.has(s) && t.opt(s).is_none() {
                continue; // an explicit null gate: the sign is never gated (burnt on v0.2)
            }
            let g = t.req(s)?;
            tab.insert(
                s.clone(),
                SignGate {
                    n2_window_nm: g.req("n2_window_nm")?.range(false)?,
                    n2_above: g.req("n2_above")?.f64()?,
                },
            );
        }
        by_transfer_key.insert(k, tab);
    }
    let unknown = n.req("unknown_transfer_key_uses")?.str()?.to_string();
    if !by_transfer_key.contains_key(&unknown) {
        return Err(n.error("unknown_transfer_key_uses must name a table"));
    }
    Ok(B6bGate {
        n2_window_nm: n.req("n2_window_nm")?.range(false)?,
        signs,
        contaminant_signs,
        by_transfer_key,
        unknown_transfer_key_uses: unknown,
    })
}

/// The soft "possible thin coating" tier of check.signs (phase 0c; a note only).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SoftTier {
    pub ester_at_least: f64,
    pub plaster_joint_load_at_least: f64,
    /// It fires only when none of these fired.
    pub requires_no_hard_sign: Vec<String>,
    /// Classes always eligible; others only when N2(otherwise_n2_window_nm) <= otherwise_n2_at_most.
    pub always_classes: Vec<String>,
    pub otherwise_n2_window_nm: (f64, f64),
    pub otherwise_n2_at_most: f64,
}

fn soft_tier(n: &Node) -> PResult<SoftTier> {
    let el = n.req("eligibility")?;
    Ok(SoftTier {
        ester_at_least: n.req("ester")?.req("at_least")?.f64()?,
        plaster_joint_load_at_least: n.req("plaster")?.req("joint_load_at_least")?.f64()?,
        requires_no_hard_sign: n.req("requires_no_hard_sign")?.vec_str()?,
        always_classes: el.req("always_classes")?.vec_str()?,
        otherwise_n2_window_nm: el.req("otherwise_n2_window_nm")?.range(false)?,
        otherwise_n2_at_most: el.req("otherwise_n2_at_most")?.f64()?,
    })
}

/// The ZooMS vote (phase 0c): `from_verdict` becomes `to_verdict` when `band` is readable and u > lit_if_u_above.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ZoomsVote {
    pub band: String,
    pub lit_if_u_above: f64,
    pub from_verdict: String,
    pub to_verdict: String,
}

fn frac(n: &Node) -> PResult<f64> {
    let x = n.f64()?;
    if x >= 0.0 {
        Ok(x)
    } else {
        Err(n.error("must be >= 0"))
    }
}

/// B9 prototypes (loaded sidecar) plus the window list and cut.
#[derive(Debug, Clone)]
pub struct B9Variant {
    pub windows_nm: Vec<(f64, f64)>,
    pub prototypes: Array,
    pub fail_if_score_below: f64,
}

fn b9_variant(n: &Node, base: &Path, windows: Option<Vec<(f64, f64)>>) -> PResult<B9Variant> {
    let windows_nm = match windows {
        Some(w) => w,
        None => ranges(&n.req("windows_nm")?)?,
    };
    let p = n.req("prototypes")?;
    let name = p.req("sidecar")?.str()?;
    let sha = p.req("sha256")?.str()?;
    let shape: Vec<usize> = p
        .req("shape")?
        .arr()?
        .iter()
        .map(|x| x.usize())
        .collect::<PResult<_>>()?;
    if let Some(d) = p.opt("dtype") {
        if d.str()? != "float64" {
            return Err(d.error("prototypes must be float64"));
        }
    }
    let a = load_sidecar(base, name, sha)?;
    if a.shape != shape {
        return Err(PluginError::new(
            ErrorKind::Sidecar,
            format!(
                "sidecar {name} has shape {:?}, the file declares {shape:?}",
                a.shape
            ),
        ));
    }
    // one prototype value per grid point in the windows (1 nm grid, inclusive)
    let points: usize = windows_nm
        .iter()
        .map(|(a, b)| ((b - a).round() as usize) + 1)
        .sum();
    if shape.len() != 2 || shape[1] != points {
        return Err(PluginError::new(
            ErrorKind::Sidecar,
            format!(
                "sidecar {name}: {} values per prototype, the windows hold {points} points",
                shape.get(1).copied().unwrap_or(0)
            ),
        ));
    }
    let cut = n.req("fail_if_score_below")?.f64()?;
    Ok(B9Variant {
        windows_nm,
        prototypes: a,
        fail_if_score_below: cut,
    })
}

/// One B8 splice-step fit (line on the SWIR1 side, extrapolated one channel across the join).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct B8Fit {
    pub fit_nm: (f64, f64),
    pub extrapolate_to_nm: f64,
    pub compare_nm: f64,
    pub denominator_nm: f64,
}

/// The Step 1 supported-input matrix.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InputMatrix {
    pub file_magic: Vec<String>,
    pub data_type_raw: u64,
    pub require_dark_corrected: bool,
    pub grid_start_nm: f64,
    pub grid_step_nm: f64,
    pub grid_n: usize,
    pub reference_range_nm: (f64, f64),
}

/// C1 coefficients (full or truncated variant).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct C1Variant {
    pub ch_bands_nm: Vec<f64>,
    pub nh_bands_nm: Vec<f64>,
    pub a: f64,
    pub b: f64,
    pub fires_if_excess_above: f64,
    /// The soft line of the "possible thin coating" tier (phase 0c), if any.
    pub soft_excess_at_least: Option<f64>,
}

fn c1_variant(n: &Node) -> PResult<C1Variant> {
    let ch = n.req("ch_bands_nm")?.vec_f64()?;
    let nh = n.req("nh_bands_nm")?.vec_f64()?;
    if ch.is_empty() || nh.is_empty() {
        return Err(n.error("C1 needs C-H and N-H bands"));
    }
    Ok(C1Variant {
        ch_bands_nm: ch,
        nh_bands_nm: nh,
        a: n.req("a")?.f64()?,
        b: n.req("b")?.f64()?,
        fires_if_excess_above: n.req("fires_if_excess_above")?.f64()?,
        soft_excess_at_least: n
            .opt("soft_tier")
            .map(|t| t.req("excess_at_least").and_then(|x| x.f64()))
            .transpose()?,
    })
}

/// Parameters of each built-in check.
#[derive(Debug, Clone)]
pub enum CheckParams {
    Acquisition {
        integrity_range_nm: (f64, f64),
        accepted_joins_nm: Vec<f64>,
        b2_dn_at_least: f64,
        b3_r_at_most: f64,
        b4_panel_mean_range: (f64, f64),
        b4_panel_sd_below: f64,
        b4_empty_mean_below: f64,
        b5_mean_r_below: f64,
        b6_check_if_implied_sd_above: f64,
        /// The per-sign B6b gates (phase 0c).
        b6b: B6bGate,
        b7_range_nm: (f64, f64),
        b7_max_r_above: f64,
        b8_note_above: f64,
        b8_check_above: f64,
        /// Per header join: (fit_nm lo, hi; extrapolate_to_nm; compare_nm; denominator_nm).
        b8_fits: Vec<B8Fit>,
        input_matrix: InputMatrix,
    },
    B9 {
        kernel: Kernel,
        std_epsilon: f64,
        full: B9Variant,
        truncated: Option<B9Variant>,
    },
    C1 {
        kernel: Kernel,
        read_half_width_nm: u64,
        full: C1Variant,
        truncated: Option<C1Variant>,
        runs_only_if_not_fired: Vec<String>,
    },
    EvidenceLevels {
        kernel: Kernel,
        core_bands: Vec<String>,
        extra_band: String,
        extra_band_faint_u: f64,
        sd_max_u: f64,
        unreadable_only_if_u_below: f64,
        clear_u: f64,
        strong_u: f64,
        guard_window_nm: (f64, f64),
        guard_max_e_below: f64,
        must_be_readable: Vec<String>,
        min_readable_core: u64,
        extra_band_max_u: f64,
        nh_type_bands: Vec<String>,
        nh_type_min_u: f64,
        clear_min_lit_core: u64,
    },
    Heat {
        edge50_scan_from_nm: f64,
        edge50_reference_nm: (f64, f64),
        edge50_fraction: f64,
        edge50_at_least_nm: f64,
        oh1433: (Sharp, f64),
        oh979: (Sharp, f64),
        r_vis_range_nm: (f64, f64),
        r_vis_at_least: f64,
        kernel_ref: String,
    },
    Longwave {
        applies_to_classes: Vec<String>,
        n2_window_nm: (f64, f64),
        tail_unreliable_if_n2_above: f64,
        /// C1's own cut (phase 0c; the contaminant check only); defaults to tail_unreliable_if_n2_above.
        c1_tail_unreliable_if_n2_above: f64,
        /// "truncated" or "not_assessed" per check (b9, c1)
        b9_mode: String,
        c1_mode: String,
    },
    Signs {
        kernel: Kernel,
        plaster_bands: Vec<(Sharp, f64, f64)>,
        plaster_fires_if_joint_load_above: f64,
        wax_all_of: Vec<(Sharp, f64)>,
        ester: (Sharp, f64),
        blocks_lift: Vec<String>,
        soft_tier: Option<SoftTier>,
    },
    ZoomsPatterns {
        kernel: Kernel,
        protein_bands: Vec<String>,
        ch_bands: Vec<String>,
        lit_if_e_above: f64,
        sd_max_frac_w: f64,
        unreadable_only_if_e_below_frac_w: f64,
        vote: Option<ZoomsVote>,
    },
}

impl CheckParams {
    /// Band names (bands.json keys) the check reads, for the cross-file check.
    pub fn band_refs(&self) -> Vec<String> {
        match self {
            CheckParams::EvidenceLevels {
                core_bands,
                extra_band,
                must_be_readable,
                nh_type_bands,
                ..
            } => core_bands
                .iter()
                .chain(std::iter::once(extra_band))
                .chain(must_be_readable)
                .chain(nh_type_bands)
                .cloned()
                .collect(),
            CheckParams::ZoomsPatterns {
                protein_bands,
                ch_bands,
                vote,
                ..
            } => protein_bands
                .iter()
                .chain(ch_bands)
                .chain(vote.iter().map(|v| &v.band))
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// A loaded engine-check parameter file.
#[derive(Debug, Clone)]
pub struct EngineCheck {
    pub header: Header,
    pub check: String,
    pub params: CheckParams,
    /// Golden cases present (run in Phase 3).
    pub golden_cases: usize,
}

const CHECKS: [&str; 8] = [
    "acquisition",
    "b9",
    "c1",
    "evidence_levels",
    "heat",
    "longwave",
    "signs",
    "zooms_patterns",
];

/// Parse and validate an engine-check parameter file (`base` = its folder, for sidecars).
pub fn parse_engine_check(doc: &Node, base: &Path) -> PResult<EngineCheck> {
    let header = parse_header(doc, ENGINE_CHECK_FORMAT)?;
    let id_ok = header
        .id
        .bytes()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'.');
    if !id_ok {
        return Err(PluginError::schema(format!(
            "id {:?} must match ^[a-z0-9_.]+$",
            header.id
        )));
    }
    let check = doc.req("check")?.one_of(&CHECKS)?.to_string();
    doc.req("title")?.str()?;
    doc.req("licence")?.str()?;
    doc.req("provenance")?.obj()?;
    let p = doc.req("parameters")?;
    p.obj()?;
    let params = match check.as_str() {
        "acquisition" => {
            let im = p.req("input_matrix")?;
            let b4 = p.req("B4_panel_or_empty")?;
            let b6b = b6b_gate(&p.req("B6b_signs_gate")?)?;
            let b7 = p.req("B7_above_one")?;
            let b8 = p.req("B8_splice_step")?;
            if b8.req("fits")?.arr()?.is_empty() {
                return Err(b8.error("fits: at least one join fit"));
            }
            CheckParams::Acquisition {
                integrity_range_nm: p.req("integrity_range_nm")?.range(false)?,
                accepted_joins_nm: im.req("accepted_joins_nm")?.vec_f64()?,
                b2_dn_at_least: p.req("B2_saturation")?.req("dn_at_least")?.f64()?,
                b3_r_at_most: p.req("B3_impossible")?.req("r_at_most")?.f64()?,
                b4_panel_mean_range: b4.req("panel_mean_range")?.range(false)?,
                b4_panel_sd_below: b4.req("panel_sd_below")?.f64()?,
                b4_empty_mean_below: b4.req("empty_mean_below")?.f64()?,
                b5_mean_r_below: p.req("B5_dark")?.req("mean_R_below")?.f64()?,
                b6_check_if_implied_sd_above: p
                    .req("B6_noise")?
                    .req("check_if_implied_sd_above")?
                    .f64()?,
                b6b,
                b7_range_nm: b7.req("range_nm")?.range(false)?,
                b7_max_r_above: b7.req("max_R_above")?.f64()?,
                b8_note_above: frac(&b8.req("note_above")?)?,
                b8_check_above: frac(&b8.req("check_above")?)?,
                b8_fits: b8
                    .req("fits")?
                    .arr()?
                    .iter()
                    .map(|f| {
                        Ok(B8Fit {
                            fit_nm: f.req("fit_nm")?.range(false)?,
                            extrapolate_to_nm: f.req("extrapolate_to_nm")?.f64()?,
                            compare_nm: f.req("compare_nm")?.f64()?,
                            denominator_nm: f.req("denominator_nm")?.f64()?,
                        })
                    })
                    .collect::<PResult<_>>()?,
                input_matrix: {
                    let g = im.req("grid")?;
                    InputMatrix {
                        file_magic: im.req("file_magic")?.vec_str()?,
                        data_type_raw: im.req("data_type_raw")?.u64()?,
                        require_dark_corrected: im
                            .opt("require_dark_corrected")
                            .map(|b| b.bool())
                            .transpose()?
                            .unwrap_or(false),
                        grid_start_nm: g.req("start_nm")?.f64()?,
                        grid_step_nm: g.req("step_nm")?.f64()?,
                        grid_n: g.req("n")?.usize()?,
                        reference_range_nm: im.req("reference_range_nm")?.range(false)?,
                    }
                },
            }
        }
        "b9" => {
            let full = b9_variant(&p, base, None)?;
            let truncated = match p.opt("truncated") {
                None => None,
                Some(t) => Some(b9_variant(&t, base, None)?),
            };
            let eps = p.req("std_epsilon")?.f64()?;
            if !(eps >= 0.0) {
                return Err(p.error("std_epsilon must be >= 0"));
            }
            CheckParams::B9 {
                kernel: kernel(&p.req("kernel")?)?,
                std_epsilon: eps,
                full,
                truncated,
            }
        }
        "c1" => CheckParams::C1 {
            kernel: kernel(&p.req("kernel")?)?,
            read_half_width_nm: p.req("read_half_width_nm")?.u64()?,
            full: c1_variant(&p)?,
            truncated: p.opt("truncated").map(|t| c1_variant(&t)).transpose()?,
            runs_only_if_not_fired: p.req("runs_only_if_not_fired")?.vec_str()?,
        },
        "evidence_levels" => {
            let r = p.req("readability")?;
            let t = p.req("thresholds_u")?;
            let g = p.req("guard")?;
            let nr = p.req("none_rule")?;
            let core_bands = band_names(&p.req("core_bands")?)?;
            if core_bands.is_empty() {
                return Err(p.error("core_bands: at least one band"));
            }
            CheckParams::EvidenceLevels {
                kernel: kernel(&p.req("kernel")?)?,
                core_bands,
                extra_band: p.req("extra_band")?.str()?.to_string(),
                extra_band_faint_u: p.req("extra_band_faint_u")?.f64()?,
                sd_max_u: frac(&r.req("sd_max_u")?)?,
                unreadable_only_if_u_below: r.req("unreadable_only_if_u_below")?.f64()?,
                clear_u: t.req("clear")?.f64()?,
                strong_u: t.req("strong")?.f64()?,
                guard_window_nm: g.req("window_nm")?.range(false)?,
                guard_max_e_below: g.req("max_E_below")?.f64()?,
                must_be_readable: band_names(&nr.req("must_be_readable")?)?,
                min_readable_core: nr.req("min_readable_core")?.u64()?,
                extra_band_max_u: nr.req("extra_band_max_u")?.f64()?,
                nh_type_bands: band_names(&p.req("nh_type_bands")?)?,
                nh_type_min_u: p.req("nh_type_min_u")?.f64()?,
                clear_min_lit_core: p.req("clear_min_lit_core")?.u64()?,
            }
        }
        "heat" => {
            let ch = p.req("charred")?;
            let e50 = ch.req("edge50")?;
            let ca = p.req("calcined")?;
            let pair = |k: &str| -> PResult<(Sharp, f64)> {
                let n = ca.req(k)?;
                Ok((sharp(&n.req("sharp")?)?, n.req("at_least")?.f64()?))
            };
            let rv = ca.req("R_vis")?;
            CheckParams::Heat {
                edge50_scan_from_nm: e50.req("scan_from_nm")?.f64()?,
                edge50_reference_nm: e50.req("reference_nm")?.range(false)?,
                edge50_fraction: e50.req("fraction")?.f64()?,
                edge50_at_least_nm: ch.req("edge50_at_least_nm")?.f64()?,
                oh1433: pair("OH1433")?,
                oh979: pair("OH979")?,
                r_vis_range_nm: rv.req("range_nm")?.range(false)?,
                r_vis_at_least: rv.req("at_least")?.f64()?,
                kernel_ref: p
                    .opt("kernel")
                    .map(|k| k.str().map(str::to_string))
                    .transpose()?
                    .unwrap_or_else(|| "signs".into()),
            }
        }
        "longwave" => {
            let w = p.req("when_unreliable")?;
            for (k, _) in w.entries()? {
                if k != "b9" && k != "c1" {
                    return Err(w.error(format!("unexpected key {k:?} (only b9 and c1)")));
                }
            }
            let modes = ["truncated", "not_assessed"];
            let cut = p.req("tail_unreliable_if_n2_above")?.f64()?;
            CheckParams::Longwave {
                applies_to_classes: p.req("applies_to_classes")?.vec_str()?,
                n2_window_nm: p.req("n2_window_nm")?.range(false)?,
                tail_unreliable_if_n2_above: cut,
                c1_tail_unreliable_if_n2_above: p
                    .opt("c1_tail_unreliable_if_n2_above")
                    .map(|x| x.f64())
                    .transpose()?
                    .unwrap_or(cut),
                b9_mode: w.req("b9")?.one_of(&modes)?.to_string(),
                c1_mode: w.req("c1")?.one_of(&modes)?.to_string(),
            }
        }
        "signs" => {
            let pl = p.req("plaster")?;
            let plaster_bands = pl
                .req("bands")?
                .arr()?
                .iter()
                .map(|b| {
                    let c = b.req("plaster_contrast")?.f64()?;
                    if c == 0.0 {
                        return Err(b.error("plaster_contrast must not be 0"));
                    }
                    Ok((sharp(&b.req("sharp")?)?, b.req("fauna_median")?.f64()?, c))
                })
                .collect::<PResult<Vec<_>>>()?;
            if plaster_bands.is_empty() {
                return Err(pl.error("bands: at least one"));
            }
            let wax_all_of = p
                .req("wax")?
                .req("all_of")?
                .arr()?
                .iter()
                .map(|w| Ok((sharp(&w.req("sharp")?)?, w.req("at_least")?.f64()?)))
                .collect::<PResult<Vec<_>>>()?;
            let es = p.req("ester")?;
            p.req("burnt")?.obj()?;
            p.req("gate")?.str()?;
            CheckParams::Signs {
                kernel: kernel(&p.req("kernel")?)?,
                plaster_bands,
                plaster_fires_if_joint_load_above: pl.req("fires_if_joint_load_above")?.f64()?,
                wax_all_of,
                ester: (sharp(&es.req("sharp")?)?, es.req("at_least")?.f64()?),
                blocks_lift: p
                    .opt("blocks_lift")
                    .map(|b| b.vec_str())
                    .transpose()?
                    .unwrap_or_default(),
                soft_tier: p.opt("soft_tier").map(|t| soft_tier(&t)).transpose()?,
            }
        }
        "zooms_patterns" => {
            let r = p.req("readability")?;
            CheckParams::ZoomsPatterns {
                kernel: kernel(&p.req("kernel")?)?,
                protein_bands: band_names(&p.req("protein_bands")?)?,
                ch_bands: band_names(&p.req("ch_bands")?)?,
                lit_if_e_above: p.req("lit_if_E_above")?.f64()?,
                sd_max_frac_w: frac(&r.req("sd_max_frac_w")?)?,
                unreadable_only_if_e_below_frac_w: r
                    .req("unreadable_only_if_E_below_frac_w")?
                    .f64()?,
                vote: match p.opt("vote") {
                    None => None,
                    Some(v) => {
                        let band = v.req("band")?.str()?.to_string();
                        band_name_ok(&v, &band)?;
                        Some(ZoomsVote {
                            band,
                            lit_if_u_above: v.req("lit_if_u_above")?.f64()?,
                            from_verdict: v.req("from_verdict")?.str()?.to_string(),
                            to_verdict: v.req("to_verdict")?.str()?.to_string(),
                        })
                    }
                },
            }
        }
        other => return Err(p.error(format!("unknown check {other:?}"))),
    };
    let golden_cases = check_deferred_block(&doc.req("golden")?)?;
    Ok(EngineCheck {
        header,
        check,
        params,
        golden_cases,
    })
}
