//! Step 10: one CSV row per scan (UTF-8 with BOM, CRLF) and the analysis manifest (port of
//! `reference/oracle/export.py`). Every number is the signed, unrounded float64 written as Python's `repr`
//! (round-trip exact); note and flag columns hold KEYS (the UI holds the strings). The ZooMS band check (the
//! band-pattern verdict with its 1545 nm vote; no longer a verdict of its own in the app, DECISIONS 80 amended) is
//! exported as `zooms_band_check.*`, and `zooms_line` holds the ZooMS line's note key (empty when there is none).

use super::val::{py_json, strs, Obj, V};
use crate::pyfmt::float_repr;

pub const CITATION: &str =
    "SPYDER Bone; collagen models after Ryder et al. 2026, J. Archaeol. Sci. 185:106448 \
                            (doi:10.1016/j.jas.2025.106448)";

fn get<'a>(o: &'a Obj, k: &str) -> &'a V {
    static NULL: V = V::Null;
    o.get(k).unwrap_or(&NULL)
}

fn obj<'a>(o: &'a Obj, k: &str) -> &'a Obj {
    static EMPTY: Obj = Obj(Vec::new());
    o.get(k).and_then(V::as_obj).unwrap_or(&EMPTY)
}

fn keys_of(list: &V) -> Vec<String> {
    match list {
        V::List(l) => l
            .iter()
            .filter_map(|n| n.as_obj().and_then(|o| o.get("key")).and_then(V::as_str))
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// The ordered row for one scan and one analysis type (the Step 10 column list).
pub fn csv_row(rec: &Obj, analysis: &str) -> Obj {
    let inp = obj(rec, "input");
    let ins = obj(rec, "instrument");
    let st = obj(rec, "stream");
    let a = obj(rec, analysis);
    let mut row = Obj::new();
    row.set("file", get(inp, "file").clone());
    row.set("input_sha256", get(inp, "input_sha256").clone());
    row.set("time_ole", get(inp, "time_ole").clone());
    row.set("time_local", get(inp, "time_local").clone());
    row.set("utc_offset_min", get(inp, "utc_offset_min").clone());
    // Python: ins.get("serial", inp.get("serial")): the instrument's value when the key exists
    row.set(
        "serial",
        ins.get("serial")
            .cloned()
            .unwrap_or_else(|| get(inp, "serial").clone()),
    );
    row.set("instrument_class", get(ins, "class").clone());
    row.set("class_source", get(ins, "class_source").clone());
    row.set("serial_known_class", get(ins, "serial_known_class").clone());
    row.set(
        "instrument_notes",
        py_json(&strs(&keys_of(get(ins, "notes"))), false),
    );
    row.set("probe_setup_note", "");
    row.set("transfer", get(st, "transfer").clone());
    row.set("transfer_sha256", get(st, "transfer_sha256").clone());
    row.set("transfer_provisional", get(st, "provisional").clone());
    row.set("analysis", analysis);
    row.set("profile", get(obj(rec, "profiles"), analysis).clone());
    row.set("verdict", get(a, "verdict").clone());
    row.set("rule_step", get(a, "rule_step").clone());
    row.set("model_verdict", get(a, "model_verdict").clone());
    row.set(
        "notes_shown",
        py_json(
            &a.get("notes_shown").cloned().unwrap_or(V::List(vec![])),
            false,
        ),
    );
    let all_keys = keys_of(get(a, "notes_all"));
    row.set("notes_all", py_json(&strs(&all_keys), false));
    row.set(
        "zooms_line",
        all_keys
            .iter()
            .find(|k| k.starts_with("zooms_"))
            .cloned()
            .unwrap_or_default(),
    );
    let flags: Vec<String> = match get(a, "flags") {
        V::List(l) => l
            .iter()
            .filter_map(V::as_obj)
            .map(|f| {
                let signs: Vec<String> = match get(f, "signs") {
                    V::List(s) => s.iter().filter_map(V::as_str).map(str::to_string).collect(),
                    _ => Vec::new(),
                };
                format!(
                    "{}:{}",
                    get(f, "key").as_str().unwrap_or(""),
                    signs.join(",")
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    row.set("flags", py_json(&strs(&flags), false));
    row.set("sort_group", get(a, "sort_group").clone());
    row.set("sort_value", get(a, "sort_value").clone());
    for (k, m) in &obj(rec, "models").0 {
        let Some(m) = m.as_obj() else { continue };
        row.set(&format!("{k}.id"), get(m, "id").clone());
        row.set(&format!("{k}.sha256"), get(m, "sha256").clone());
        row.set(&format!("{k}.value"), get(m, "value").clone());
        row.set(&format!("{k}.transfer"), get(m, "transfer").clone());
        row.set(&format!("{k}.status"), get(m, "status").clone());
        row.set(&format!("{k}.implied_sd"), get(m, "implied_sd").clone());
        row.set(&format!("{k}.B6"), get(obj(m, "B6"), "outcome").clone());
        row.set(&format!("{k}.domain_ratio"), get(m, "domain_ratio").clone());
        for (c, v) in &obj(m, "components").0 {
            row.set(&format!("{k}.{c}"), v.clone());
        }
    }
    let ev = obj(rec, "evidence");
    for k in ["level", "S", "n_readable_core", "n_lit_core", "guard_max_E"] {
        row.set(&format!("evidence.{k}"), get(ev, k).clone());
    }
    for (b, d) in &obj(ev, "bands").0 {
        let Some(d) = d.as_obj() else { continue };
        for k in ["E", "u", "sd_u", "state"] {
            row.set(&format!("band.{b}.{k}"), get(d, k).clone());
        }
    }
    let zo = obj(rec, "zooms_pattern");
    row.set("zooms_band_check.pattern", get(zo, "pattern").clone());
    row.set("zooms_band_check.verdict", get(zo, "verdict").clone());
    row.set(
        "zooms_band_check.pattern_verdict",
        get(zo, "pattern_verdict").clone(),
    );
    row.set("zooms_band_check.vote_1545", get(zo, "vote_1545").clone());
    for (b, d) in &obj(zo, "bands").0 {
        let Some(d) = d.as_obj() else { continue };
        row.set(&format!("zband.{b}.lit"), get(d, "lit").clone());
        row.set(&format!("zband.{b}.readable"), get(d, "readable").clone());
    }
    if let Some(vb) = zo.get("vote_band").and_then(V::as_obj) {
        let b = get(vb, "id").as_str().unwrap_or("").to_string();
        for k in ["u", "sd_u", "lit", "readable"] {
            row.set(&format!("zband.{b}.{k}"), get(vb, k).clone());
        }
    }
    for (k, c) in &obj(rec, "checks").0 {
        let Some(c) = c.as_obj() else { continue };
        for (kk, vv) in &c.0 {
            let v = match vv {
                V::List(_) => V::Str(py_json(vv, false)),
                other => other.clone(),
            };
            row.set(&format!("check.{k}.{kk}"), v);
        }
    }
    for (k, s) in &obj(rec, "signs").0 {
        let Some(s) = s.as_obj() else { continue };
        for (kk, vv) in &s.0 {
            let v = match vv {
                V::Map(_) => V::Str(py_json(vv, true)),
                V::List(_) => V::Str(py_json(vv, false)),
                other => other.clone(),
            };
            row.set(&format!("sign.{k}.{kk}"), v);
        }
    }
    for (kk, vv) in &obj(rec, "C1").0 {
        row.set(&format!("sign.C1.{kk}"), vv.clone());
    }
    for (w, v) in &obj(rec, "n2").0 {
        row.set(&format!("n2.{w}"), v.clone());
    }
    row.set("oracle_version", get(rec, "oracle_version").clone());
    let deps: Vec<String> = obj(rec, "dependencies")
        .0
        .iter()
        .map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or("")))
        .collect();
    row.set("dependencies", deps.join(";"));
    row.set("citation", CITATION);
    row
}

/// One CSV cell (the oracle's `_cell`): None -> "", booleans "true"/"false", floats by `repr` (non-finite
/// -> ""), everything else `str()`.
pub fn cell(v: &V) -> String {
    match v {
        V::Null => String::new(),
        V::Bool(b) => if *b { "true" } else { "false" }.into(),
        V::Num(x) if x.is_finite() => float_repr(*x),
        V::Num(_) => String::new(),
        V::Int(i) => i.to_string(),
        V::Str(s) => s.clone(),
        other => py_json(other, false),
    }
}

/// Python `csv` QUOTE_MINIMAL quoting.
fn quote(s: &str) -> String {
    if s.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// The CSV text (UTF-8 with BOM, CRLF): the header is the union of the rows' columns in first-seen order.
pub fn csv_text(rows: &[Obj]) -> String {
    let mut cols: Vec<String> = Vec::new();
    for r in rows {
        for (k, _) in &r.0 {
            if !cols.contains(k) {
                cols.push(k.clone());
            }
        }
    }
    let line = |cells: Vec<String>| -> String {
        // Python writes a lone empty field as "" so the row is not mistaken for a blank line
        if cells.len() == 1 && cells[0].is_empty() {
            "\"\"\r\n".to_string()
        } else {
            let mut s = cells.iter().map(|c| quote(c)).collect::<Vec<_>>().join(",");
            s.push_str("\r\n");
            s
        }
    };
    let mut out = String::from("\u{feff}");
    out.push_str(&line(cols.clone()));
    for r in rows {
        out.push_str(&line(
            cols.iter()
                .map(|c| r.get(c).map(cell).unwrap_or_default())
                .collect(),
        ));
    }
    out
}

/// The analysis manifest of one result (input byte hash, effective configuration, every dependency hash,
/// result revision).
pub fn manifest(rec: &Obj, analysis: &str, revision: i64) -> Obj {
    let ins = obj(rec, "instrument");
    Obj::new()
        .with("format", "spyder-bone/analysis_manifest")
        .with("format_version", 1i64)
        .with(
            "input_sha256",
            get(obj(rec, "input"), "input_sha256").clone(),
        )
        .with(
            "effective_configuration",
            Obj::new()
                .with("instrument_class", get(ins, "class").clone())
                .with("class_source", get(ins, "class_source").clone())
                .with("analysis", analysis)
                .with("profile", get(obj(rec, "profiles"), analysis).clone())
                .with("transfer", get(obj(rec, "stream"), "transfer").clone()),
        )
        .with("dependencies", V::Map(obj(rec, "dependencies").clone()))
        .with(
            "engine",
            Obj::new()
                .with("spyder_core", env!("CARGO_PKG_VERSION"))
                .with("oracle", get(rec, "oracle_version").clone()),
        )
        .with("result_revision", revision)
}
