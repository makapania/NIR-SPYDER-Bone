"""Step 10: one CSV row per scan (UTF-8 with BOM) and the analysis manifest. Every number is the signed, unrounded
float64 (repr, round-trip exact); the display policy is the UI's job. Note and flag columns hold KEYS (the UI holds
the strings). The ZooMS band check (the band-pattern verdict with its 1545 nm vote; DECISIONS 80 amended: no longer
a verdict of its own in the app) is exported as `zooms_band_check.*`, and `zooms_line` holds the ZooMS line's note key
(empty when there is none).
"""
from __future__ import annotations

import csv
import io
import json
import math

CITATION = ("SPYDER Bone; collagen models after Ryder et al. 2026, J. Archaeol. Sci. 185:106448 "
            "(doi:10.1016/j.jas.2025.106448)")


def flatten(rec, prefix=""):
    """Dotted-key flat view of a result record; list items get their index as a key and the list its length as
    '<key>.n', so every leaf is a scalar (numbers compare with a tolerance, labels exactly)."""
    out = {}
    if isinstance(rec, dict):
        for k, v in rec.items():
            out.update(flatten(v, f"{prefix}{k}."))
        return out
    key = prefix[:-1]
    if isinstance(rec, (list, tuple)):
        out[f"{key}.n"] = len(rec)
        for i, v in enumerate(rec):
            out.update(flatten(v, f"{key}.{i}."))
    else:
        out[key] = rec
    return out


def _cell(v):
    if v is None:
        return ""
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, float):
        return repr(v) if math.isfinite(v) else ""
    return str(v)


def csv_row(rec, analysis):
    """Ordered dict for one scan and one analysis type (Step 10 column list)."""
    inp, ins, st = rec.get("input", {}), rec.get("instrument", {}), rec.get("stream", {})
    a = rec.get(analysis, {})
    row = {
        "file": inp.get("file"), "input_sha256": inp.get("input_sha256"), "time_ole": inp.get("time_ole"),
        "time_local": inp.get("time_local"), "utc_offset_min": inp.get("utc_offset_min"),
        "serial": ins.get("serial", inp.get("serial")), "instrument_class": ins.get("class"),
        "class_source": ins.get("class_source"), "serial_known_class": ins.get("serial_known_class"),
        "instrument_notes": json.dumps([n["key"] for n in ins.get("notes", [])]), "probe_setup_note": "",
        "transfer": st.get("transfer"), "transfer_sha256": st.get("transfer_sha256"),
        "transfer_provisional": st.get("provisional"),
        "analysis": analysis, "profile": rec.get("profiles", {}).get(analysis),
        "verdict": a.get("verdict"), "rule_step": a.get("rule_step"), "model_verdict": a.get("model_verdict"),
        "notes_shown": json.dumps(a.get("notes_shown", [])),
        "notes_all": json.dumps([n["key"] for n in a.get("notes_all", [])]),
        "zooms_line": next((n["key"] for n in a.get("notes_all", []) if n["key"].startswith("zooms_")), ""),
        "flags": json.dumps([f["key"] + ":" + ",".join(f["signs"]) for f in a.get("flags", [])]),
        "sort_group": a.get("sort_group"), "sort_value": a.get("sort_value"),
    }
    for k, m in rec.get("models", {}).items():
        row.update({f"{k}.id": m.get("id"), f"{k}.sha256": m.get("sha256"), f"{k}.value": m.get("value"),
                    f"{k}.transfer": m.get("transfer"), f"{k}.status": m.get("status"),
                    f"{k}.implied_sd": m.get("implied_sd"), f"{k}.B6": (m.get("B6") or {}).get("outcome"),
                    f"{k}.domain_ratio": m.get("domain_ratio")})
        for c, v in (m.get("components") or {}).items():
            row[f"{k}.{c}"] = v
    ev = rec.get("evidence", {})
    row.update({"evidence.level": ev.get("level"), "evidence.S": ev.get("S"),
                "evidence.n_readable_core": ev.get("n_readable_core"), "evidence.n_lit_core": ev.get("n_lit_core"),
                "evidence.guard_max_E": ev.get("guard_max_E")})
    for b, d in (ev.get("bands") or {}).items():
        row.update({f"band.{b}.E": d["E"], f"band.{b}.u": d["u"], f"band.{b}.sd_u": d["sd_u"], f"band.{b}.state": d["state"]})
    zo = rec.get("zooms_pattern", {})
    row.update({"zooms_band_check.pattern": zo.get("pattern"), "zooms_band_check.verdict": zo.get("verdict"),
                "zooms_band_check.pattern_verdict": zo.get("pattern_verdict"),
                "zooms_band_check.vote_1545": zo.get("vote_1545")})
    for b, d in (zo.get("bands") or {}).items():
        row.update({f"zband.{b}.lit": d["lit"], f"zband.{b}.readable": d["readable"]})
    vb = zo.get("vote_band")
    if vb:
        b = vb["id"]
        row.update({f"zband.{b}.u": vb["u"], f"zband.{b}.sd_u": vb["sd_u"], f"zband.{b}.lit": vb["lit"],
                    f"zband.{b}.readable": vb["readable"]})
    for k, c in rec.get("checks", {}).items():
        for kk, vv in c.items():
            row[f"check.{k}.{kk}"] = json.dumps(vv) if isinstance(vv, list) else vv
    for k, s in rec.get("signs", {}).items():
        for kk, vv in s.items():
            row[f"sign.{k}.{kk}"] = (json.dumps(vv, sort_keys=True) if isinstance(vv, dict)
                                     else json.dumps(vv) if isinstance(vv, list) else vv)
    for kk, vv in rec.get("C1", {}).items():
        row[f"sign.C1.{kk}"] = vv
    for w, v in rec.get("n2", {}).items():
        row[f"n2.{w}"] = v
    row["oracle_version"] = rec.get("oracle_version")
    row["dependencies"] = ";".join(f"{k}={v}" for k, v in rec.get("dependencies", {}).items())
    row["citation"] = CITATION
    return row


def write_csv(rows, path):
    cols = []
    for r in rows:
        for k in r:
            if k not in cols:
                cols.append(k)
    buf = io.StringIO()
    w = csv.writer(buf, lineterminator="\r\n")
    w.writerow(cols)
    for r in rows:
        w.writerow([_cell(r.get(c)) for c in cols])
    with open(path, "w", encoding="utf-8-sig", newline="") as f:
        f.write(buf.getvalue())


def manifest(rec, analysis, revision=1):
    ins = rec.get("instrument", {})
    return {"format": "spyder-bone/analysis_manifest", "format_version": 1,
            "input_sha256": rec.get("input", {}).get("input_sha256"),
            "effective_configuration": {"instrument_class": ins.get("class"), "class_source": ins.get("class_source"),
                                        "analysis": analysis, "profile": rec.get("profiles", {}).get(analysis),
                                        "transfer": rec.get("stream", {}).get("transfer")},
            "dependencies": rec.get("dependencies", {}), "engine": {"oracle": rec.get("oracle_version")},
            "result_revision": revision}
