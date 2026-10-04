//! The core's record (the oracle structure, `spyder analyse --json`) mapped to the UI's `ScanResult`
//! (`ui/src/lib/types.ts`). Faithful: every verdict, rule step, reading, band state, note key and flag comes
//! from the record; nothing is recomputed here except unit conversions the UI needs to draw (u = E / w per
//! band) and name changes (core keys -> UI keys).

use serde::Serialize;
use spyder_core::model::Body;
use spyder_core::pipeline::val::{Obj, V};
use spyder_core::plugins::checks::CheckParams;
use spyder_core::plugins::CLASS_HIRES;

use crate::engine::Core;
use crate::session::{ui_class, ClassSource, Entry, Input};

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    /// "assessed", "not_assessed", "gated" or "skipped".
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// A qualifier on an assessed result (e.g. "truncated (long-wave region too noisy)").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The core's status text -> the UI's assessment.
pub fn assessment(s: Option<&str>) -> Assessment {
    let s = s.unwrap_or("not assessed: no result");
    let (head, tail) = match s.split_once(':') {
        Some((h, t)) => (h.trim(), Some(t.trim().to_string())),
        None => (s.trim(), None),
    };
    match head {
        "assessed" => Assessment {
            status: "assessed",
            reason: None,
            note: tail,
        },
        "gated" => Assessment {
            status: "gated",
            reason: tail,
            note: None,
        },
        h if h.starts_with("skipped") => Assessment {
            status: "skipped",
            reason: tail.or_else(|| Some(s.to_string())),
            note: None,
        },
        _ => Assessment {
            status: "not_assessed",
            reason: tail.or_else(|| Some(s.to_string())),
            note: None,
        },
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferInfo {
    pub id: String,
    pub version: String,
    pub provisional: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub key: String,
    pub params: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelReading {
    pub key: &'static str,
    pub id: String,
    pub version: String,
    pub value: Option<f64>,
    pub status: Assessment,
    pub domain_note: bool,
    /// Noise propagated to this reading (B6), % collagen; None when not assessed.
    pub implied_sd: Option<f64>,
    /// B6 "Check": the noise widens this reading's error.
    pub noise_check: bool,
    /// What it reads: "transferred (id@version)", "as measured", ...
    pub transfer: Option<String>,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Models {
    pub cons3: Option<ModelReading>,
    pub wc2045: Option<ModelReading>,
    pub wc1500: Option<ModelReading>,
    pub f05: Option<ModelReading>,
    pub ryder2045: Option<ModelReading>,
    pub s1r2: Option<ModelReading>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BandReading {
    pub id: &'static str,
    pub nm: f64,
    pub e: f64,
    pub u: f64,
    pub readable: bool,
    pub lit: bool,
    /// Display state (lit / flat / can't tell, as the ZooMS rule reads the band; lit bands graded by u).
    pub state: &'static str,
    /// The organic-evidence rule's own state for this band (Step 7; "faint" below its faint threshold),
    /// None for C-H 2284 (not an evidence band).
    pub evidence_state: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub level: &'static str,
    pub s: Option<f64>,
    pub bands: Vec<BandReading>,
    pub status: Assessment,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Zooms {
    pub pattern: Option<String>,
    pub lit_count: usize,
    pub readable_count: usize,
    /// The ZooMS band check's call (Good / Borderline / Unlikely / Can't tell, after the 1545 nm vote), UI form:
    /// supporting evidence in the details; the verdict itself is the radiocarbon / isotopes one (DECISIONS 80 amended).
    pub verdict: &'static str,
    /// The OH-corrected 1545 nm band voted Unlikely up to Borderline (DECISIONS 75).
    pub vote1545: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sign {
    pub id: &'static str,
    pub fired: bool,
    pub status: Assessment,
    /// Heat sign only: where the visible edge reaches half the 1250–1300 nm reflectance (charred when late).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge50_nm: Option<f64>,
    /// Heat sign only: "charred" or "calcined" (either implies fired); what the spectrum marks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heat_kind: Option<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub id: &'static str,
    pub outcome: &'static str,
    pub status: Assessment,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Flag {
    pub key: String,
    pub signs: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub scan_id: String,
    pub file: String,
    pub path: String,
    pub acquired_at: String,
    pub serial: Option<u64>,
    pub instrument_class: &'static str,
    pub class_source: &'static str,
    pub transfer: Option<TransferInfo>,
    pub analysis: String,
    pub profile_id: String,
    pub verdict: &'static str,
    pub verdict_rule: &'static str,
    /// The core's rule step, verbatim (exports use it).
    pub rule_step: String,
    pub model_verdict: Option<&'static str>,
    pub notes: Vec<Note>,
    /// The (at most two) notes shown under the verdict, chosen by the core's precedence.
    pub notes_shown: Vec<String>,
    pub flags: Vec<Flag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unusable_reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unusable_detail: Option<String>,
    pub models: Models,
    pub evidence: Evidence,
    pub zooms: Zooms,
    pub signs: Vec<Sign>,
    pub checks: Vec<CheckResult>,
    pub altered_oh_band: bool,
    pub engine_version: String,
    /// The profile's "most promising first" key (unrounded).
    pub sort_group: Option<f64>,
    pub sort_value: Option<f64>,
    pub input_sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arrived_seq: Option<u64>,
    pub file_revision: u32,
    pub scan_kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind_detail: Option<String>,
    /// Milliseconds the analysis took.
    pub score_ms: f64,
}

fn obj<'a>(o: &'a Obj, k: &str) -> Option<&'a Obj> {
    o.get(k).and_then(V::as_obj)
}

fn s<'a>(o: Option<&'a Obj>, k: &str) -> Option<&'a str> {
    o.and_then(|o| o.get(k)).and_then(V::as_str)
}

fn f(o: Option<&Obj>, k: &str) -> Option<f64> {
    o.and_then(|o| o.get(k))
        .and_then(V::as_f64)
        .filter(|x| x.is_finite())
}

fn to_json(v: &V) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

pub fn ui_verdict(v: &str) -> &'static str {
    match v {
        "Good" => "good",
        "Borderline" => "borderline",
        "Unlikely" => "unlikely",
        "Can't tell" => "cant_tell",
        "Doesn't look like bone" => "not_bone",
        _ => "rescan",
    }
}

fn ui_rule(step: &str) -> &'static str {
    match step {
        "model_verdict" => "model",
        "flat_bands_lead" => "flat_bands",
        "plus_d" => "plus_d",
        "lift_good" => "lift_good",
        "lift_borderline" => "lift_borderline",
        "lift_blocked" => "lift_blocked",
        "zooms_pattern" => "zooms_pattern",
        "B9" => "not_bone",
        "no_model_reading" => "no_model_reading",
        _ => "unusable",
    }
}

/// Core sign names -> UI sign ids.
pub fn ui_sign(n: &str) -> &'static str {
    match n {
        "plaster" => "plaster",
        "wax" => "wax",
        "ester" => "ester",
        "burnt" => "burnt",
        _ => "c1",
    }
}

fn ui_level(l: Option<&str>) -> &'static str {
    match l {
        Some("none") => "none",
        Some("trace") => "trace",
        Some("clear") => "clear",
        Some("strong") => "strong",
        _ => "cant_tell",
    }
}

fn ui_evidence_state(st: &str) -> &'static str {
    match st {
        "strong" => "strong",
        "clear" => "clear",
        "faint" => "faint",
        "flat" => "flat",
        _ => "cant_tell",
    }
}

/// Core note key -> UI note key (the UI joins the two blocked-lift keys and renames flat_bands_lead).
pub fn ui_note_key(k: &str) -> String {
    match k {
        "flat_bands_lead" => "flat_bands".into(),
        "lift_blocked_contaminant" | "lift_blocked_noisy" => "lift_blocked".into(),
        other => other.into(),
    }
}

fn note(n: &Obj, set_class: &str, serial: Option<u64>) -> Note {
    let key = n.get("key").and_then(V::as_str).unwrap_or("").to_string();
    let mut p = serde_json::Map::new();
    let num = |k: &str| n.get(k).and_then(V::as_f64);
    let mut put = |k: &str, v: serde_json::Value| {
        p.insert(k.to_string(), v);
    };
    if let Some(m) = num("m") {
        put("m", m.into());
    }
    match key.as_str() {
        "lift_blocked_contaminant" => {
            put("strength", to_json(n.get("level").unwrap_or(&V::Null)));
            let first = match n.get("signs") {
                Some(V::List(l)) => l
                    .iter()
                    .filter_map(V::as_str)
                    .map(ui_sign)
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };
            put("sign", first.first().copied().unwrap_or("c1").into());
            put("signs", first.join(",").into());
        }
        "lift_blocked_noisy" => {
            put("strength", to_json(n.get("level").unwrap_or(&V::Null)));
            put("reason", "noise".into());
        }
        "models_disagree" => {
            if let Some(c) = obj(n, "components") {
                for (k, name) in [("wc2045", "a"), ("wc1500", "b"), ("F05", "c")] {
                    if let Some(x) = c.get(k).and_then(V::as_f64) {
                        put(name, x.into());
                    }
                }
            }
        }
        "contaminants_not_checked" => {
            // per-sign gates (DECISIONS 76): which contaminant signs were skipped for noise, which were checked
            for k in ["skipped", "checked"] {
                if let Some(V::List(l)) = n.get(k) {
                    let ids: Vec<serde_json::Value> = l
                        .iter()
                        .filter_map(V::as_str)
                        .map(|x| ui_sign(x).into())
                        .collect();
                    put(k, serde_json::Value::Array(ids));
                }
            }
        }
        "zooms_better_ryder" => {
            // the published Ryder 2045 reading and the profile's ZooMS cut
            for k in ["r", "at_least"] {
                if let Some(x) = num(k) {
                    put(k, x.into());
                }
            }
        }
        "zooms_faint_protein" => {
            // the lit N-type bands, as UI band ids (the text names 2175 when it is the only one)
            if let Some(V::List(l)) = n.get("lit") {
                let ids: Vec<serde_json::Value> = l
                    .iter()
                    .filter_map(V::as_str)
                    .map(|b| {
                        match b {
                            "NH2044" => "nh2044",
                            "AM2175" => "amide2175",
                            "NH1545c" => "nh1545",
                            other => other,
                        }
                        .into()
                    })
                    .collect();
                put("lit", serde_json::Value::Array(ids));
            }
        }
        "possible_thin_coating" => {
            if let Some(V::List(l)) = n.get("components") {
                let c: Vec<serde_json::Value> =
                    l.iter().filter_map(V::as_str).map(|x| x.into()).collect();
                put("components", serde_json::Value::Array(c));
            }
        }
        "second_opinion_differs" => {
            if let Some(x) = num("s1") {
                put("s", x.into());
            }
        }
        "serial_class_mismatch" => {
            if let Some(sr) = serial {
                put("serial", sr.into());
            }
            put(
                "known",
                ui_class(n.get("serial_class").and_then(V::as_str).unwrap_or("")).into(),
            );
            put("set", ui_class(set_class).into());
        }
        _ => {}
    }
    Note {
        key: ui_note_key(&key),
        params: p,
    }
}

fn checks(rec: &Obj, cons3: Option<&ModelReading>) -> Vec<CheckResult> {
    let Some(c) = obj(rec, "checks") else {
        return Vec::new();
    };
    let outcome = |o: Option<&str>| -> &'static str {
        match o.map(str::to_ascii_lowercase).as_deref() {
            Some("ok") | Some("pass") => "ok",
            Some("note") => "note",
            Some("check") => "check",
            Some("unusable") | Some("fail") => "unusable",
            _ => "ok",
        }
    };
    let mut out = Vec::new();
    for (k, id) in [
        ("B1", "b1"),
        ("B2", "b2"),
        ("B3", "b3"),
        ("B4", "b4"),
        ("B5", "b5"),
        ("B6b", "b6b"),
        ("B7", "b7"),
        ("B8", "b8"),
        ("B9", "b9"),
    ] {
        let Some(x) = obj(c, k) else { continue };
        let value = match k {
            "B6b" => f(Some(x), "n2"),
            "B9" => f(Some(x), "score"),
            "B5" => f(Some(x), "mean_R"),
            "B7" => f(Some(x), "max_R"),
            "B2" => f(Some(x), "max_dn"),
            _ => None,
        };
        out.push(CheckResult {
            id,
            outcome: outcome(s(Some(x), "outcome")),
            status: assessment(s(Some(x), "status")),
            value,
            detail: s(Some(x), "reason").map(str::to_string),
        });
    }
    if let Some(m) = cons3 {
        out.push(CheckResult {
            id: "b6",
            outcome: if m.noise_check { "check" } else { "ok" },
            status: if m.implied_sd.is_some() {
                assessment(Some("assessed"))
            } else {
                assessment(Some(
                    "not assessed: no noise gain for this model and transfer",
                ))
            },
            value: m.implied_sd,
            detail: None,
        });
    }
    if let Some(x) = obj(c, "longwave") {
        let unrel = x.get("tail_unreliable").and_then(V::as_bool) == Some(true);
        out.push(CheckResult {
            id: "longwave",
            outcome: if unrel { "note" } else { "ok" },
            status: assessment(s(Some(x), "status")),
            value: f(Some(x), "n2"),
            detail: None,
        });
    }
    out
}

fn model_reading(key: &'static str, m: &Obj) -> ModelReading {
    let id_ver = s(Some(m), "id").unwrap_or("");
    let (id, version) = id_ver.rsplit_once('@').unwrap_or((id_ver, ""));
    let sd = f(Some(m), "implied_sd");
    ModelReading {
        key,
        id: id.to_string(),
        version: version.to_string(),
        value: f(Some(m), "value"),
        status: assessment(s(Some(m), "status")),
        domain_note: m.get("domain_note").and_then(V::as_bool) == Some(true),
        implied_sd: sd,
        noise_check: s(obj(m, "B6"), "outcome") == Some("Check"),
        transfer: s(Some(m), "transfer").map(str::to_string),
    }
}

fn models(core: &Core, rec: &Obj) -> Models {
    let mut out = Models::default();
    let Some(ms) = obj(rec, "models") else {
        return out;
    };
    if let Some(c) = obj(ms, "cons3") {
        let base = model_reading("cons3", c);
        // the components: their identities from the consensus file, their values and noise from the record
        let comps = core.registry().and_then(|r| {
            r.model(&base.id).and_then(|m| match &m.body {
                Body::Consensus { components } => Some(components.clone()),
                _ => None,
            })
        });
        for (name, key) in [("wc2045", "wc2045"), ("wc1500", "wc1500"), ("F05", "f05")] {
            let value = obj(c, "components")
                .and_then(|o| o.get(name))
                .and_then(V::as_f64);
            let sd = f(obj(c, "component_sd"), name);
            let (id, version) = comps
                .as_ref()
                .and_then(|cs| cs.iter().find(|x| x.name == name))
                .map(|x| (x.id.clone(), x.version.to_string()))
                .unwrap_or_else(|| (name.to_string(), String::new()));
            let r = ModelReading {
                key,
                id,
                version,
                value: value.filter(|x| x.is_finite()),
                status: if value.is_some_and(f64::is_finite) {
                    base.status.clone()
                } else {
                    assessment(Some("not assessed: no reading"))
                },
                domain_note: false,
                implied_sd: sd,
                noise_check: false,
                transfer: base.transfer.clone(),
            };
            match key {
                "wc2045" => out.wc2045 = Some(r),
                "wc1500" => out.wc1500 = Some(r),
                _ => out.f05 = Some(r),
            }
        }
        out.cons3 = Some(base);
    }
    if let Some(m) = obj(ms, "ryder2045") {
        out.ryder2045 = Some(model_reading("ryder2045", m));
    }
    if let Some(m) = obj(ms, "s1_r2") {
        out.s1r2 = Some(model_reading("s1r2", m));
    }
    out
}

/// The six bands in the UI's order (bands.ts) with their weights.
const BANDS: [(&str, &str, f64); 6] = [
    ("CH1689", "ch1689", 1689.0),
    ("CH1728", "ch1728", 1728.0),
    ("NH2044", "nh2044", 2044.0),
    ("AM2175", "amide2175", 2175.0),
    ("CH2262", "ch2262", 2262.0),
    ("CH2284", "ch2284", 2284.0),
];

fn evidence(core: &Core, rec: &Obj) -> (Evidence, Zooms) {
    let ev = obj(rec, "evidence");
    let zo = obj(rec, "zooms_pattern");
    let reg = core.registry();
    let (clear_u, strong_u) = reg
        .and_then(|r| {
            r.checks()
                .into_iter()
                .find(|c| c.check == "evidence_levels")
                .and_then(|c| match &c.params {
                    CheckParams::EvidenceLevels {
                        clear_u, strong_u, ..
                    } => Some((*clear_u, *strong_u)),
                    _ => None,
                })
        })
        .unwrap_or((1.5, 4.0));
    let weight = |b: &str| {
        reg.and_then(|r| r.bands())
            .and_then(|t| t.bands.get(b))
            .and_then(|x| x.weight)
            .unwrap_or(f64::NAN)
    };
    let zb = obj_or_none(zo, "bands");
    let eb = obj_or_none(ev, "bands");
    let mut bands = Vec::new();
    if zb.is_some_and(|z| !z.0.is_empty()) {
        for (core_id, ui_id, nm) in BANDS {
            let z = zb.and_then(|z| obj(z, core_id));
            let e = f(z, "E").unwrap_or(f64::NAN);
            let w = weight(core_id);
            let u = e / w;
            let readable = z.and_then(|z| z.get("readable")).and_then(V::as_bool) == Some(true);
            let lit = z.and_then(|z| z.get("lit")).and_then(V::as_bool) == Some(true);
            let state = if !readable {
                "cant_tell"
            } else if !lit {
                "flat"
            } else if u >= strong_u {
                "strong"
            } else if u >= clear_u {
                "clear"
            } else {
                "trace"
            };
            bands.push(BandReading {
                id: ui_id,
                nm,
                e,
                u,
                readable,
                lit,
                state,
                evidence_state: s(eb.and_then(|b| obj(b, core_id)), "state").map(ui_evidence_state),
            });
        }
    }
    // The OH-corrected 1545 nm band (DECISIONS 75): an evidence core band (lit from 1.5 u) and a ZooMS vote; its u
    // comes from the record (the band is a trough, u = -E / w), its readability from the ZooMS vote block.
    if let Some(b) = eb.and_then(|b| obj(b, "NH1545c")) {
        let vb = obj_or_none(zo, "vote_band");
        let mut ev_state = s(Some(b), "state").map(ui_evidence_state);
        let readable = vb
            .and_then(|v| v.get("readable"))
            .and_then(V::as_bool)
            .unwrap_or(ev_state != Some("cant_tell"));
        // the drawn state follows the ZooMS vote's lit line (1.25 u), like the six bands follow the ZooMS rule; a
        // band lit for ZooMS but under the evidence line (1.5 u) shows as 'faint' in the evidence grid
        let lit = vb
            .and_then(|v| v.get("lit"))
            .and_then(V::as_bool)
            .unwrap_or(matches!(ev_state, Some("clear") | Some("strong")));
        if lit && ev_state == Some("flat") {
            ev_state = Some("faint");
        }
        let u = f(Some(b), "u").unwrap_or(f64::NAN);
        bands.push(BandReading {
            id: "nh1545",
            nm: 1545.0,
            e: f(Some(b), "E").unwrap_or(f64::NAN),
            u,
            readable,
            lit,
            state: if !readable {
                "cant_tell"
            } else if !lit {
                "flat"
            } else if u >= strong_u {
                "strong"
            } else if u >= clear_u {
                "clear"
            } else {
                "trace"
            },
            evidence_state: ev_state,
        });
    }
    let evd = Evidence {
        level: ui_level(s(ev, "level")),
        s: f(ev, "S"),
        bands,
        status: assessment(s(ev, "status").or(Some("not assessed: scan not analysed"))),
    };
    let zcount = |k: &str| {
        zb.map(|z| {
            z.0.iter()
                .filter(|(_, b)| {
                    b.as_obj().and_then(|o| o.get(k)).and_then(V::as_bool) == Some(true)
                })
                .count()
        })
        .unwrap_or(0)
    };
    let zv = Zooms {
        pattern: s(zo, "pattern").map(str::to_string),
        lit_count: zcount("lit"),
        readable_count: zcount("readable"),
        verdict: ui_verdict(s(zo, "verdict").unwrap_or("Can't tell")),
        vote1545: zo.and_then(|z| z.get("vote_1545")).is_some_and(V::truthy),
    };
    (evd, zv)
}

fn obj_or_none<'a>(o: Option<&'a Obj>, k: &str) -> Option<&'a Obj> {
    o.and_then(|o| obj(o, k))
}

fn signs(rec: &Obj) -> Vec<Sign> {
    let sg = obj(rec, "signs");
    let mut out = Vec::new();
    for n in ["plaster", "wax", "ester", "burnt"] {
        if let Some(x) = sg.and_then(|o| obj(o, n)) {
            let heat = n == "burnt";
            let truthy = |k: &str| x.get(k).is_some_and(V::truthy);
            out.push(Sign {
                id: ui_sign(n),
                fired: truthy("fired"),
                status: assessment(s(Some(x), "status")),
                edge50_nm: if heat { f(Some(x), "edge50_nm") } else { None },
                heat_kind: match (heat, truthy("charred"), truthy("calcined")) {
                    (true, true, _) => Some("charred"),
                    (true, false, true) => Some("calcined"),
                    _ => None,
                },
            });
        }
    }
    if let Some(x) = obj(rec, "C1") {
        out.push(Sign {
            id: "c1",
            fired: x.get("fired").is_some_and(V::truthy),
            status: assessment(s(Some(x), "status")),
            edge50_nm: None,
            heat_kind: None,
        });
    }
    out
}

fn acquired_at(rec: &Obj, e: &Entry) -> String {
    let inp = obj(rec, "input");
    match (
        s(inp, "time_local"),
        inp.and_then(|i| i.get("utc_offset_min"))
            .and_then(V::as_f64),
    ) {
        (Some(t), Some(off)) => {
            let off = off as i64;
            let sign = if off < 0 { '-' } else { '+' };
            format!("{t}{sign}{:02}:{:02}", off.abs() / 60, off.abs() % 60)
        }
        (Some(t), None) => t.to_string(),
        _ => modified_iso(e.modified_ms),
    }
}

fn modified_iso(ms: Option<i64>) -> String {
    let Some(ms) = ms else { return String::new() };
    let secs = ms.div_euclid(1000);
    let (d, s) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (y, mo, da) = spyder_core::timefmt::civil_from_days(d);
    format!(
        "{y:04}-{mo:02}-{da:02}T{:02}:{:02}:{:02}Z",
        s / 3600,
        (s / 60) % 60,
        s % 60
    )
}

fn unusable(rec: &Obj) -> (Option<&'static str>, Option<String>) {
    let c = obj(rec, "checks");
    let out = |k: &str| s(c.and_then(|c| obj(c, k)), "outcome") == Some("Unusable");
    if out("B2") {
        return (Some("saturated"), None);
    }
    if out("B4") {
        let mean = f(c.and_then(|c| obj(c, "B4")), "mean_R").unwrap_or(0.0);
        return (Some(if mean >= 0.5 { "panel" } else { "low_signal" }), None);
    }
    if out("B3") {
        return (Some("low_signal"), None);
    }
    let reason = s(c.and_then(|c| obj(c, "B1")), "reason").map(str::to_string);
    (Some("unreadable"), reason)
}

/// The UI's result for one scan and one analysis type.
pub fn scan_result(
    core: &Core,
    e: &Entry,
    analysis: &str,
    class: &str,
    source: ClassSource,
) -> ScanResult {
    let mut r = ScanResult {
        scan_id: e.id.clone(),
        file: e.file.clone(),
        path: e.path.clone(),
        acquired_at: modified_iso(e.modified_ms),
        serial: None,
        instrument_class: ui_class(class),
        class_source: source.ui_str(),
        transfer: None,
        analysis: analysis.to_string(),
        profile_id: String::new(),
        verdict: "rescan",
        verdict_rule: "unusable",
        rule_step: String::new(),
        model_verdict: None,
        notes: Vec::new(),
        notes_shown: Vec::new(),
        flags: Vec::new(),
        unusable_reason: None,
        unusable_detail: None,
        models: Models::default(),
        evidence: Evidence {
            level: "cant_tell",
            s: None,
            bands: Vec::new(),
            status: assessment(Some("not assessed: scan not analysed")),
        },
        zooms: Zooms {
            pattern: None,
            lit_count: 0,
            readable_count: 0,
            verdict: "cant_tell",
            vote1545: false,
        },
        signs: Vec::new(),
        checks: Vec::new(),
        altered_oh_band: false,
        engine_version: format!(
            "spyder-core {} (oracle {})",
            env!("CARGO_PKG_VERSION"),
            spyder_core::pipeline::ORACLE_VERSION
        ),
        sort_group: None,
        sort_value: None,
        input_sha256: None,
        arrived_seq: e.arrived_seq,
        file_revision: e.revision,
        scan_kind: "sample",
        kind_detail: None,
        score_ms: e.score_ms,
    };
    let Some(rec) = e.record.as_deref() else {
        // not scored: an unreadable file, or no verdict model (the startup-error state)
        match &e.input {
            Input::Unreadable(why) => {
                r.scan_kind = "unreadable";
                r.kind_detail = Some(why.clone());
            }
            _ => {
                r.scan_kind = "unscored";
                r.kind_detail = Some(match &core.error {
                    Some(err) => format!("No verdict model available: {err}"),
                    None => "not analysed".to_string(),
                });
            }
        }
        return r;
    };
    let inp = obj(rec, "input");
    let ins = obj(rec, "instrument");
    r.input_sha256 = s(inp, "input_sha256").map(str::to_string);
    r.serial = f(ins, "serial")
        .or_else(|| f(inp, "serial"))
        .filter(|x| *x > 0.0)
        .map(|x| x as u64);
    r.acquired_at = acquired_at(rec, e);
    if let Some(t) = s(obj(rec, "stream"), "transfer").filter(|t| *t != "none") {
        let (id, version) = t.rsplit_once('@').unwrap_or((t, ""));
        r.transfer = Some(TransferInfo {
            id: id.to_string(),
            version: version.to_string(),
            provisional: obj(rec, "stream")
                .and_then(|o| o.get("provisional"))
                .and_then(V::as_bool)
                == Some(true),
        });
    }
    r.profile_id = s(obj(rec, "profiles"), analysis).unwrap_or("").to_string();
    let a = obj(rec, analysis);
    let verdict = s(a, "verdict").unwrap_or("Rescan");
    r.rule_step = s(a, "rule_step").unwrap_or("").to_string();
    match verdict {
        "Not scored" => {
            r.scan_kind = "reference";
            r.kind_detail = s(obj_or_none(obj(rec, "checks"), "B1"), "reason").map(str::to_string);
            return r;
        }
        "Unsupported" => {
            r.scan_kind = "unreadable";
            r.kind_detail = s(obj_or_none(obj(rec, "checks"), "B1"), "reason").map(str::to_string);
            return r;
        }
        _ => {}
    }
    r.verdict = ui_verdict(verdict);
    r.verdict_rule = ui_rule(&r.rule_step);
    r.model_verdict = s(a, "model_verdict").map(ui_verdict);
    r.sort_group = f(a, "sort_group");
    r.sort_value = f(a, "sort_value");
    if r.verdict == "rescan" {
        let (why, detail) = unusable(rec);
        r.unusable_reason = why;
        r.unusable_detail = detail;
    }
    let set_class = s(ins, "class").unwrap_or(class);
    if let Some(V::List(l)) = a.and_then(|a| a.get("notes_all")) {
        r.notes = l
            .iter()
            .filter_map(V::as_obj)
            .map(|n| note(n, set_class, r.serial))
            .collect();
    }
    if let Some(V::List(l)) = ins.and_then(|i| i.get("notes")) {
        r.notes.extend(
            l.iter()
                .filter_map(V::as_obj)
                .map(|n| note(n, set_class, r.serial)),
        );
    }
    // An unlisted serial whose header SWIR gains suggest the other class: a gentle details note (a heuristic;
    // a listed serial has the core's serial note instead).
    let listed = r.serial.and_then(|sr| core.class_of_serial(sr)).is_some();
    if let Some(h) = e.hint.filter(|h| !listed && *h != set_class) {
        let mut p = serde_json::Map::new();
        p.insert("known".into(), ui_class(h).into());
        p.insert("set".into(), ui_class(set_class).into());
        r.notes.push(Note {
            key: "header_class_mismatch".into(),
            params: p,
        });
    }
    if let Some(V::List(l)) = a.and_then(|a| a.get("notes_shown")) {
        r.notes_shown = l.iter().filter_map(V::as_str).map(ui_note_key).collect();
    }
    if let Some(V::List(l)) = a.and_then(|a| a.get("flags")) {
        r.flags = l
            .iter()
            .filter_map(V::as_obj)
            .map(|fl| Flag {
                key: s(Some(fl), "key").unwrap_or("").to_string(),
                signs: match fl.get("signs") {
                    Some(V::List(sg)) => sg.iter().filter_map(V::as_str).map(ui_sign).collect(),
                    _ => Vec::new(),
                },
            })
            .collect();
    }
    r.models = models(core, rec);
    // S1_R2 (the transfer-free 1500 model) is a high-res second opinion only (PLAN Step 6)
    if class != CLASS_HIRES {
        r.models.s1r2 = None;
    }
    let (ev, zo) = evidence(core, rec);
    r.evidence = ev;
    r.zooms = zo;
    r.signs = signs(rec);
    // a flag shows only for a sign the profile flags (every v1 profile flags all five)
    let flagged: Vec<&str> = r
        .flags
        .iter()
        .flat_map(|f| f.signs.iter().copied())
        .collect();
    for sg in &mut r.signs {
        sg.fired = sg.fired && flagged.contains(&sg.id);
    }
    r.checks = checks(rec, r.models.cons3.as_ref());
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_assessments() {
        assert_eq!(assessment(Some("assessed")).status, "assessed");
        let t = assessment(Some("assessed: truncated (long-wave region too noisy)"));
        assert_eq!(t.status, "assessed");
        assert_eq!(
            t.note.as_deref(),
            Some("truncated (long-wave region too noisy)")
        );
        let g = assessment(Some("gated: B6b too noisy for contaminant signs"));
        assert_eq!(g.status, "gated");
        assert_eq!(
            g.reason.as_deref(),
            Some("B6b too noisy for contaminant signs")
        );
        assert_eq!(
            assessment(Some("not assessed: long-wave region too noisy")).status,
            "not_assessed"
        );
        assert_eq!(
            assessment(Some("skipped: a specific sign fired")).status,
            "skipped"
        );
        assert_eq!(assessment(None).status, "not_assessed");
    }

    #[test]
    fn verdicts_rules_and_notes_are_renamed_faithfully() {
        assert_eq!(ui_verdict("Doesn't look like bone"), "not_bone");
        assert_eq!(ui_verdict("Can't tell"), "cant_tell");
        assert_eq!(ui_rule("flat_bands_lead"), "flat_bands");
        assert_eq!(ui_rule("B2+B4"), "unusable");
        assert_eq!(ui_note_key("lift_blocked_noisy"), "lift_blocked");
        assert_eq!(
            ui_note_key("positive_signs_all_six"),
            "positive_signs_all_six"
        );
        assert_eq!(ui_sign("C1"), "c1");
    }
}
