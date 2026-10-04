//! Analysis profiles (`spyder-bone/analysis_profile`, PLAN section 4 and Step 9): radiocarbon / isotopes
//! (rule L2: cuts 0.5 / 3, flat-bands-lead, +D, lift, flags, notes, sort) and ZooMS (the band-pattern verdict).
//! Validated against `plugins/schemas/analysis_profile.schema.json`; the rules themselves are in
//! [`crate::pipeline::verdict`], and the goldens run on load (`crate::pipeline::checkgold`).

use std::collections::BTreeMap;

use serde::Serialize;

use super::{parse_header, Header, Node, PResult, PluginError, PROFILE_FORMAT};

/// One model a profile shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileModel {
    pub key: String,
    pub model_id: String,
    pub role: String,
    /// CONS3 component names (verdict input only).
    pub components: Vec<String>,
    pub shown_for_classes: Vec<String>,
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
    pub bands_too_noisy_level: Option<String>,
    /// The above_bone_range note when m exceeds this (phase 0c, DECISIONS 72); None = no such note.
    pub above_bone_range_m_above: Option<f64>,
    /// The ZooMS line (DECISIONS 80 amended): a note when the ZooMS band-pattern verdict ranks above this verdict.
    pub zooms_better: bool,
    /// Contaminant signs whose FIRING suppresses the ZooMS line (coatings can create the C-H bands it reads).
    pub zooms_better_suppressed_by: Vec<String>,
    /// The protein line's strength test (None: no test, every band-pattern Borderline is the protein line).
    pub zooms_protein: Option<ProteinTest>,
    /// The quietest notes: shown only when a slot is left after the rule, ZooMS and supporting notes.
    pub precedence_last: Vec<String>,
    /// The Ryder 2045 ZooMS line: the published model's key in the profile and its cut (None = no such line).
    pub zooms_ryder: Option<(String, f64)>,
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
    pub classes: Vec<String>,
    pub gap_points: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Sort {
    pub verdict_order: Vec<String>,
    /// verdict -> "m" | "S"
    pub within: BTreeMap<String, String>,
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
                shown_for_classes: m
                    .opt("shown_for_classes")
                    .map(|c| c.vec_str())
                    .transpose()?
                    .unwrap_or_default(),
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
    };
    let second_opinion = match p.opt("second_opinion") {
        None => None,
        Some(s) => Some(SecondOpinion {
            classes: strs(&s, "classes")?,
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
