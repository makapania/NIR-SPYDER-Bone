//! Analysis profiles (`spyder-bone/analysis_profile`, PLAN section 4 and Step 9): radiocarbon / isotopes
//! (rule L2: cuts 0.5 / 3, flat-bands-lead, +D, lift, flags, notes, sort) and ZooMS (the band-pattern verdict).
//! Validated against `plugins/schemas/analysis_profile.schema.json`; the rules themselves are in
//! [`crate::pipeline::verdict`], and the goldens run on load (`crate::pipeline::checkgold`).

use std::collections::BTreeMap;

use serde::Serialize;

use super::{parse_header, Header, Node, PResult, PluginError, PROFILE_FORMAT};

/// Which scans a profile model (or the second opinion) is for (DECISIONS 137: keyed on the stream, not on class names).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum ShownFor {
    /// Every scan.
    EveryScan,
    /// Only when the stream is not the native standard-resolution scan (a correction applied, unless near-uncorrected,
    /// or another instrument's scan as measured).
    NonNative,
    /// Legacy (before DECISIONS 137): these instrument classes.
    Classes(Vec<String>),
}

impl ShownFor {
    /// Whether it applies to a scan of this class whose stream is (or is not) the native standard-resolution scan.
    pub fn applies(&self, native: bool, class: &str) -> bool {
        match self {
            ShownFor::EveryScan => true,
            ShownFor::NonNative => !native,
            ShownFor::Classes(c) => c.iter().any(|x| x == class),
        }
    }
}

/// `shown_for` ("every_scan" | "non_native"), else a legacy class list under `legacy_key`.
fn shown_for(n: &Node, key: &str, legacy_key: &str) -> PResult<ShownFor> {
    if let Some(v) = n.opt(key) {
        return Ok(match v.one_of(&["every_scan", "non_native"])? {
            "every_scan" => ShownFor::EveryScan,
            _ => ShownFor::NonNative,
        });
    }
    Ok(ShownFor::Classes(
        n.opt(legacy_key)
            .map(|c| c.vec_str())
            .transpose()?
            .unwrap_or_default(),
    ))
}

/// One model a profile shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileModel {
    pub key: String,
    pub model_id: String,
    pub role: String,
    /// CONS3 component names (verdict input only).
    pub components: Vec<String>,
    pub shown_for: ShownFor,
    /// The model runs on the scan as measured for any instrument (`reads: "as_measured"`; S1_R2 is transfer-free).
    pub reads_as_measured: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlatBandsLead {
    pub enabled: bool,
    pub evidence_level: String,
    pub verdict: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlusD {
    pub enabled: bool,
    pub zooms_verdict: String,
    pub m_at_least: f64,
    pub verdict: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lift {
    pub requires_zooms_verdict: String,
    pub to_good_levels: Vec<String>,
    pub to_at_least_borderline_levels: Vec<String>,
    pub blocked_by_signs: Vec<String>,
    pub requires_contaminants_checked: bool,
}

/// When the models-disagree note fires (DECISIONS 65).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelsDisagreeWhen {
    /// The CONS3 components fall in different verdict bands of the profile's cuts (they straddle a cut).
    ComponentsInDifferentVerdictBands,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Notes {
    pub precedence_rule: Vec<String>,
    /// The ZooMS line's slot: after the rule's note, before the supporting note (DECISIONS 80 amended).
    pub precedence_zooms: Vec<String>,
    pub precedence_supporting: Vec<String>,
    pub max_shown: usize,
    pub models_disagree_when: ModelsDisagreeWhen,
    /// ... and only when the components span at least this many points (max - min).
    pub models_disagree_min_span: f64,
    pub positive_signs_all_six_zooms_verdict: String,
    pub positive_signs_clear_levels: Vec<String>,
    pub positive_signs_clear_min_m: f64,
    pub positive_signs_hidden_after_lift: bool,
    /// Panel notes whose firing suppresses the positive-signs line (base keys "ch_above_nh", "readers_disagree_strongly":
    /// no encouraging line against conflicting evidence; DECISIONS 128).
    pub positive_signs_suppressed_by_notes: Vec<String>,
    pub bands_too_noisy_level: Option<String>,
    /// The above_bone_range note when m exceeds this (phase 0c, DECISIONS 72); None = no such note.
    pub above_bone_range_m_above: Option<f64>,
    /// The ZooMS line (DECISIONS 80 amended): a note when the ZooMS band-pattern verdict ranks above this verdict.
    pub zooms_better: bool,
    /// Contaminant signs whose FIRING suppresses the ZooMS line (coatings can create the C-H bands it reads).
    pub zooms_better_suppressed_by: Vec<String>,
    /// Notes whose firing suppresses the ZooMS line the same way ("ch_above_nh": the C-H note, DECISIONS 120).
    pub zooms_better_suppressed_by_notes: Vec<String>,
    /// The protein line's strength test (None: no test, every band-pattern Borderline is the protein line).
    pub zooms_protein: Option<ProteinTest>,
    /// The quietest notes: shown only when a slot is left after the rule, ZooMS and supporting notes.
    pub precedence_last: Vec<String>,
    /// The strong warnings ("ch_above_nh_strong", "readers_disagree_strongly"): when one fires it is always shown, after the
    /// other notes and outside `max_shown`, and never takes the supporting slot (DECISIONS 134; like a flag, it never
    /// competes for a note's place).
    pub warnings: Vec<String>,
    /// The Ryder 2045 ZooMS line: the published model's key in the profile and its cut (None = no such line).
    pub zooms_ryder: Option<(String, f64)>,
    /// The panel notes (DECISIONS 111, 115, 119; None = no such notes).
    pub panel: Option<PanelNotes>,
}

/// The contrasts of the readers-disagree-strongly note, in the order the note lists them.
pub const PANEL_CONTRASTS: [&str; 6] = ["CH", "CHN", "GL", "SHP", "SP3", "RAW"];

/// The panel notes (DECISIONS 111, 115, 119; oracle `verdict.panel_notes`): the readers by profile model key, the C-H
/// note's two levels and the readers-disagree-strongly scales, each per CONS3 bin (`cons3_bin_edges`; bin = how many
/// edges m reaches). Notes only: they never change the verdict, the lift or the sort.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelNotes {
    /// The C-H 1728 nm reader.
    pub ch_key: String,
    /// The narrow 1545 nm contrast reader (full range).
    pub narrow1545_key: String,
    /// The published (uncorrected) Ryder 2045 model.
    pub raw2045_key: String,
    pub ch_above_nh: Option<ChAboveNh>,
    pub disagree: Option<ReadersDisagree>,
}

/// ch_above_nh_possible above `possible_above`, ch_above_nh_strong above `strong_above` (the C-H reader minus m).
#[derive(Debug, Clone, PartialEq)]
pub struct ChAboveNh {
    pub bin_edges: Vec<f64>,
    pub possible_above: Vec<f64>,
    pub strong_above: Vec<f64>,
}

/// readers_disagree_strongly: any contrast's (x - median) / (q98 - median) of m's bin above `level`, and the contrast
/// itself above `min_gap` points (a gap under a point is noise near zero; DECISIONS 133).
#[derive(Debug, Clone, PartialEq)]
pub struct ReadersDisagree {
    pub bin_edges: Vec<f64>,
    pub level: f64,
    pub min_gap: f64,
    /// (name, median per bin, q98 per bin), in [`PANEL_CONTRASTS`] order; contrasts the profile omits are absent.
    pub contrasts: Vec<(String, Vec<f64>, Vec<f64>)>,
}

/// Strictly ascending, finite bin edges.
fn bin_edges(n: &Node) -> PResult<Vec<f64>> {
    let e = n.vec_f64()?;
    if e.iter().any(|x| !x.is_finite()) || e.windows(2).any(|w| w[0] >= w[1]) {
        return Err(n.error("cons3_bin_edges: finite and strictly ascending"));
    }
    Ok(e)
}

fn parse_panel(n: &Node, models: &[ProfileModel]) -> PResult<PanelNotes> {
    let rd = n.req("readers")?;
    let key = |k: &str| -> PResult<String> {
        let v = rd.req(k)?;
        let s = v.str()?;
        if !models.iter().any(|m| m.key == s) {
            return Err(v.error(format!(
                "readers.{k}: {s:?} is not one of the profile's model keys"
            )));
        }
        Ok(s.to_string())
    };
    let (ch_key, narrow1545_key, raw2045_key) = (key("ch")?, key("narrow1545")?, key("raw2045")?);
    let ch_above_nh = match n.opt("ch_above_nh") {
        None => None,
        Some(c) => {
            let bin_edges = bin_edges(&c.req("cons3_bin_edges")?)?;
            let nb = bin_edges.len() + 1;
            let possible_above = c.req("possible_above")?.vec_f64_len(nb)?;
            let strong_above = c.req("strong_above")?.vec_f64_len(nb)?;
            if possible_above
                .iter()
                .zip(&strong_above)
                .any(|(p, s)| !(p < s))
            {
                return Err(c.error("strong_above must exceed possible_above in every bin"));
            }
            Some(ChAboveNh {
                bin_edges,
                possible_above,
                strong_above,
            })
        }
    };
    let disagree = match n.opt("readers_disagree_strongly") {
        None => None,
        Some(r) => {
            let bin_edges = bin_edges(&r.req("cons3_bin_edges")?)?;
            let nb = bin_edges.len() + 1;
            let level = r.req("level")?.f64()?;
            if !(level > 0.0) {
                return Err(r.error("level must be positive"));
            }
            let min_gap = r
                .opt("min_gap")
                .map(|x| x.f64())
                .transpose()?
                .unwrap_or(0.0);
            if !(min_gap >= 0.0) {
                return Err(r.error("min_gap must be zero or positive"));
            }
            let cs = r.req("contrasts")?;
            let mut got = Vec::new();
            for (name, c) in cs.entries()? {
                if !PANEL_CONTRASTS.contains(&name.as_str()) {
                    return Err(c.error(format!("unknown contrast {name:?}")));
                }
                let med = c.req("median")?.vec_f64_len(nb)?;
                let q98 = c.req("q98")?.vec_f64_len(nb)?;
                if med.iter().zip(&q98).any(|(m, q)| !(q > m)) {
                    return Err(c.error("q98 must exceed median in every bin"));
                }
                got.push((name, med, q98));
            }
            got.sort_by_key(|(n, _, _)| PANEL_CONTRASTS.iter().position(|p| p == n));
            Some(ReadersDisagree {
                bin_edges,
                level,
                min_gap,
                contrasts: got,
            })
        }
    };
    Ok(PanelNotes {
        ch_key,
        narrow1545_key,
        raw2045_key,
        ch_above_nh,
        disagree,
    })
}

/// The stronger protein line (Matt): m >= `m_at_least` AND at least `n_type_lit_at_least` of `n_type_bands` lit and
/// readable; otherwise the faint note. `n_type_bands` is never empty (rejected at load).
#[derive(Debug, Clone, PartialEq)]
pub struct ProteinTest {
    pub m_at_least: f64,
    pub n_type_lit_at_least: usize,
    pub n_type_bands: Vec<String>,
}

/// The ZooMS profile's notes (phase 0c): above_bone_range and possible_thin_coating, one shown by precedence;
/// c1_reduced details-only.
#[derive(Debug, Clone, PartialEq)]
pub struct ZoomsNotes {
    pub precedence_supporting: Vec<String>,
    pub max_shown: usize,
    pub above_bone_range_m_above: Option<f64>,
}

/// The rule-L2 parameters (radiocarbon, isotopes).
#[derive(Debug, Clone, PartialEq)]
pub struct RuleL2 {
    pub unlikely_below: f64,
    pub good_from: f64,
    pub flat_bands_lead: FlatBandsLead,
    pub plus_d: PlusD,
    pub lift: Lift,
    pub notes: Notes,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProfileParams {
    CollagenRuleL2(Box<RuleL2>),
    /// ZooMS: the verdict is the band-pattern verdict of this check.
    ZoomsPattern {
        verdict_from_check: String,
        notes: Option<ZoomsNotes>,
    },
}

/// Which signs raise which flag.
#[derive(Debug, Clone, PartialEq)]
pub struct Flags {
    pub contaminant: Vec<String>,
    pub burnt: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SecondOpinion {
    /// Which scans the note is for (the non-native streams).
    pub streams: ShownFor,
    /// The default gap; a correction file may carry its own (`provenance.second_opinion_gap`).
    pub gap_points: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sort {
    pub verdict_order: Vec<String>,
    /// verdict -> "m" | "S"
    pub within: BTreeMap<String, String>,
    /// DECISIONS 99: a verdict block whose flags name one of these signs sorts after the unpenalised scans of its
    /// own verdict group (`sort_penalty` 1). None: the profile has no penalty (no `sort_penalty` key is written).
    pub penalty_signs: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct AnalysisProfile {
    pub header: Header,
    pub analysis: String,
    pub kind: String,
    pub models: Vec<ProfileModel>,
    pub params: ProfileParams,
    pub flags: Flags,
    pub sort: Sort,
    pub second_opinion: Option<SecondOpinion>,
    pub domain_note_ratio_above: Option<f64>,
    pub golden_cases: usize,
}

impl AnalysisProfile {
    /// The model whose reading drives the verdict (role `verdict_input`).
    pub fn verdict_model_id(&self) -> Option<&str> {
        self.models
            .iter()
            .find(|m| m.role == "verdict_input")
            .map(|m| m.model_id.as_str())
    }
}

fn strs(n: &Node, key: &str) -> PResult<Vec<String>> {
    n.req(key)?.vec_str()
}

pub fn parse_profile(doc: &Node) -> PResult<AnalysisProfile> {
    let header = parse_header(doc, PROFILE_FORMAT)?;
    let analysis = doc
        .req("analysis")?
        .one_of(&["radiocarbon", "isotopes", "zooms"])?
        .to_string();
    doc.req("licence")?.str()?;
    doc.req("provenance")?.obj()?;
    let p = doc.req("parameters")?;
    let models: Vec<ProfileModel> = p
        .req("models")?
        .arr()?
        .iter()
        .map(|m| {
            Ok(ProfileModel {
                key: m.req("key")?.str()?.to_string(),
                model_id: m.req("model_id")?.str()?.to_string(),
                role: m.req("role")?.str()?.to_string(),
                components: m
                    .opt("components")
                    .map(|c| c.vec_str())
                    .transpose()?
                    .unwrap_or_default(),
                shown_for: shown_for(m, "shown_for", "shown_for_classes")?,
                reads_as_measured: match m.opt("reads") {
                    None => false,
                    Some(r) => r.one_of(&["as_measured"]).map(|_| true)?,
                },
            })
        })
        .collect::<PResult<_>>()?;
    if models.is_empty() {
        return Err(p.error("models: at least one"));
    }
    let fl = p.req("flags")?;
    let flags = Flags {
        contaminant: strs(&fl, "contaminant")?,
        burnt: strs(&fl, "burnt")?,
    };
    let so = p.req("sort")?;
    let sort = Sort {
        verdict_order: strs(&so, "verdict_order")?,
        within: so
            .req("within")?
            .entries()?
            .into_iter()
            .map(|(k, v)| v.one_of(&["m", "S"]).map(|s| (k, s.to_string())))
            .collect::<PResult<_>>()?,
        penalty_signs: match so.opt("penalty_signs") {
            None => None,
            Some(n) => {
                let v = n.vec_str()?;
                if let Some(bad) = v.iter().find(|s| !flags.contaminant.contains(s)) {
                    return Err(n.error(format!(
                        "sort.penalty_signs: '{bad}' is not one of the profile's contaminant flag signs"
                    )));
                }
                Some(v)
            }
        },
    };
    let second_opinion = match p.opt("second_opinion") {
        None => None,
        Some(s) => Some(SecondOpinion {
            streams: shown_for(&s, "streams", "classes")?,
            gap_points: s.req("gap_points")?.f64()?,
        }),
    };
    let domain_note_ratio_above = p
        .opt("domain_note_ratio_above")
        .map(|d| d.f64())
        .transpose()?;
    let kind = p
        .req("kind")?
        .one_of(&["collagen_rule_L2", "zooms_pattern"])?
        .to_string();
    let params = if kind == "collagen_rule_L2" {
        let cuts = p.req("cuts")?;
        let (lo, hi) = (
            cuts.req("unlikely_below")?.f64()?,
            cuts.req("good_from")?.f64()?,
        );
        if !(lo < hi) {
            return Err(cuts.error("unlikely_below must be below good_from"));
        }
        let f = p.req("flat_bands_lead")?;
        let d = p.req("plus_d")?;
        let lift = p.req("lift")?;
        let blocked = strs(&lift, "blocked_by_signs")?;
        for s in &blocked {
            if !["plaster", "wax", "ester", "C1"].contains(&s.as_str()) {
                return Err(lift.error(format!(
                    "blocked_by_signs: {s:?} is not plaster, wax, ester or C1"
                )));
            }
        }
        let notes = p.req("notes")?;
        let ps = notes.req("positive_signs")?;
        if models.iter().all(|m| m.role != "verdict_input") {
            return Err(p.error("models: one model needs role \"verdict_input\""));
        }
        ProfileParams::CollagenRuleL2(Box::new(RuleL2 {
            unlikely_below: lo,
            good_from: hi,
            flat_bands_lead: FlatBandsLead {
                enabled: f.req("enabled")?.bool()?,
                evidence_level: f.req("evidence_level")?.str()?.to_string(),
                verdict: f.req("verdict")?.str()?.to_string(),
            },
            plus_d: PlusD {
                enabled: d.req("enabled")?.bool()?,
                zooms_verdict: d.req("zooms_verdict")?.str()?.to_string(),
                m_at_least: d.req("m_at_least")?.f64()?,
                verdict: d.req("verdict")?.str()?.to_string(),
            },
            lift: Lift {
                requires_zooms_verdict: lift.req("requires_zooms_verdict")?.str()?.to_string(),
                to_good_levels: strs(&lift, "to_good_levels")?,
                to_at_least_borderline_levels: strs(&lift, "to_at_least_borderline_levels")?,
                blocked_by_signs: blocked,
                requires_contaminants_checked: lift.req("requires_contaminants_checked")?.bool()?,
            },
            notes: Notes {
                precedence_rule: strs(&notes, "precedence_rule")?,
                precedence_zooms: notes
                    .opt("precedence_zooms")
                    .map(|n| n.vec_str())
                    .transpose()?
                    .unwrap_or_default(),
                precedence_supporting: strs(&notes, "precedence_supporting")?,
                max_shown: notes.req("max_shown")?.usize()?,
                models_disagree_when: {
                    let w = notes.req("models_disagree_when")?;
                    match w.str()? {
                        "components_in_different_verdict_bands" => {
                            ModelsDisagreeWhen::ComponentsInDifferentVerdictBands
                        }
                        other => {
                            return Err(
                                w.error(format!("models_disagree_when: unknown rule {other:?}"))
                            )
                        }
                    }
                },
                models_disagree_min_span: notes.req("models_disagree_min_span")?.f64()?,
                positive_signs_all_six_zooms_verdict: ps
                    .req("all_six_zooms_verdict")?
                    .str()?
                    .to_string(),
                positive_signs_clear_levels: strs(&ps, "clear_levels")?,
                positive_signs_clear_min_m: ps.req("clear_min_m")?.f64()?,
                positive_signs_suppressed_by_notes: {
                    let s = ps
                        .opt("suppressed_by_notes")
                        .map(|n| n.vec_str())
                        .transpose()?
                        .unwrap_or_default();
                    if let Some(x) = s.iter().find(|x| {
                        !matches!(x.as_str(), "ch_above_nh" | "readers_disagree_strongly")
                    }) {
                        return Err(ps.error(format!(
                            "suppressed_by_notes: {x:?} is not ch_above_nh or readers_disagree_strongly"
                        )));
                    }
                    s
                },
                positive_signs_hidden_after_lift: notes
                    .opt("positive_signs_hidden_after_lift")
                    .map(|b| b.bool())
                    .transpose()?
                    .unwrap_or(true),
                bands_too_noisy_level: notes
                    .opt("bands_too_noisy_level")
                    .map(|b| b.str().map(str::to_string))
                    .transpose()?,
                above_bone_range_m_above: notes
                    .opt("above_bone_range_m_above")
                    .map(|b| b.f64())
                    .transpose()?,
                zooms_better: match notes.opt("zooms_better") {
                    None => false,
                    Some(z) => z.req("enabled")?.bool()?,
                },
                zooms_better_suppressed_by: match notes.opt("zooms_better") {
                    None => Vec::new(),
                    Some(z) => {
                        let s = z
                            .opt("suppressed_by_signs")
                            .map(|n| n.vec_str())
                            .transpose()?
                            .unwrap_or_default();
                        for x in &s {
                            if !["plaster", "wax", "ester", "C1"].contains(&x.as_str()) {
                                return Err(z.error(format!(
                                    "suppressed_by_signs: {x:?} is not plaster, wax, ester or C1"
                                )));
                            }
                        }
                        s
                    }
                },
                zooms_better_suppressed_by_notes: match notes.opt("zooms_better") {
                    None => Vec::new(),
                    Some(z) => {
                        let s = z
                            .opt("suppressed_by_notes")
                            .map(|n| n.vec_str())
                            .transpose()?
                            .unwrap_or_default();
                        if let Some(x) = s.iter().find(|x| x.as_str() != "ch_above_nh") {
                            return Err(
                                z.error(format!("suppressed_by_notes: {x:?} is not ch_above_nh"))
                            );
                        }
                        s
                    }
                },
                zooms_protein: match notes.opt("zooms_better").and_then(|z| z.opt("protein")) {
                    None => None,
                    Some(p) => {
                        let bands = strs(&p, "n_type_bands")?;
                        if bands.is_empty() {
                            return Err(p.error(
                                "n_type_bands: at least one band (an empty list would make every scan pass)",
                            ));
                        }
                        Some(ProteinTest {
                            m_at_least: p.req("m_at_least")?.f64()?,
                            n_type_lit_at_least: p.req("n_type_lit_at_least")?.usize()?,
                            n_type_bands: bands,
                        })
                    }
                },
                zooms_ryder: match notes
                    .opt("zooms_better")
                    .and_then(|z| z.opt("ryder_2045_at_least").map(|c| (z, c)))
                {
                    None => None,
                    Some((z, c)) => {
                        Some((z.req("ryder_2045_model_key")?.str()?.to_string(), c.f64()?))
                    }
                },
                precedence_last: notes
                    .opt("precedence_last")
                    .map(|n| n.vec_str())
                    .transpose()?
                    .unwrap_or_default(),
                warnings: {
                    let w = notes
                        .opt("warnings")
                        .map(|n| n.vec_str())
                        .transpose()?
                        .unwrap_or_default();
                    if let Some(x) = w.iter().find(|x| {
                        !matches!(
                            x.as_str(),
                            "ch_above_nh_strong" | "readers_disagree_strongly"
                        )
                    }) {
                        return Err(notes.error(format!(
                            "warnings: {x:?} is not ch_above_nh_strong or readers_disagree_strongly"
                        )));
                    }
                    w
                },
                panel: notes
                    .opt("panel")
                    .map(|n| parse_panel(&n, &models))
                    .transpose()?,
            },
        }))
    } else {
        ProfileParams::ZoomsPattern {
            verdict_from_check: p.req("verdict_from_check")?.str()?.to_string(),
            notes: match p.opt("notes") {
                None => None,
                Some(n) => Some(ZoomsNotes {
                    precedence_supporting: strs(&n, "precedence_supporting")?,
                    max_shown: n.req("max_shown")?.usize()?,
                    above_bone_range_m_above: n
                        .opt("above_bone_range_m_above")
                        .map(|b| b.f64())
                        .transpose()?,
                }),
            },
        }
    };
    if analysis == "zooms" && !matches!(params, ProfileParams::ZoomsPattern { .. }) {
        return Err(PluginError::schema(
            "the ZooMS profile must use the band-pattern verdict (kind zooms_pattern; DECISIONS 17)",
        ));
    }
    let golden_cases = super::golden::check_deferred_block(&doc.req("golden")?)?;
    Ok(AnalysisProfile {
        header,
        analysis,
        kind,
        models,
        params,
        flags,
        sort,
        second_opinion,
        domain_note_ratio_above,
        golden_cases,
    })
}
