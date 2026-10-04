"""Validate a plug-in folder as the app would on load: JSON Schema (if jsonschema is installed), structural checks,
goldens (engine tolerance 1e-9), consensus component hashes, catalog hashes.

    python -m spyder_ref.validate_plugins ../plugins            # exit code 1 on any failure

Formats handled here: model (incl. consensus), transfer, bands, noise_gains, instrument, catalog, golden spectra.
Engine-check files and analysis profiles: schema (one per check kind) and goldens through reference/oracle/checkgold.
All JSON schemas live in plugins/schemas/.
"""
from __future__ import annotations
import json
import sys
from pathlib import Path
from . import core as SR
from . import tables as TB

SCHEMA_DIR = Path(__file__).resolve().parents[2] / "plugins" / "schemas"     # all schemas live in one place
SCHEMA_FOR = {"spyder-bone/model": "spyder-model.schema.json", "spyder-bone/transfer": "spyder-transfer.schema.json",
              "spyder-bone/bands": "spyder-bands.schema.json", "spyder-bone/noise_gains": "spyder-noise-gains.schema.json",
              "spyder-bone/instrument": "spyder-instrument.schema.json", "spyder-bone/catalog": "spyder-catalog.schema.json",
              "spyder-bone/golden_spectra": "spyder-golden-spectra.schema.json",
              "spyder-bone/reference_set": "spyder-reference-set.schema.json",
              "spyder-bone/engine_check": None, "spyder-bone/analysis_profile": "analysis_profile.schema.json"}


def _schema_file(doc):
    """engine-check files have one schema per check kind (engine_check.<check>.schema.json)."""
    if doc["format"] == "spyder-bone/engine_check":
        return f"engine_check.{doc.get('check')}.schema.json"
    return SCHEMA_FOR[doc["format"]]


def _schema_validator():
    try:
        from jsonschema import Draft202012Validator
        from referencing import Registry, Resource
    except ImportError:
        return None
    schemas = {f.name: json.loads(f.read_text(encoding="utf-8")) for f in SCHEMA_DIR.glob("*.schema.json")}
    reg = Registry().with_resources([(s["$id"], Resource.from_contents(s)) for s in schemas.values()]
                                    + [(f, Resource.from_contents(s)) for f, s in schemas.items()])
    for s in schemas.values():
        Draft202012Validator.check_schema(s)
    return lambda doc: list(Draft202012Validator(schemas[_schema_file(doc)], registry=reg).iter_errors(doc))


def validate_folder(folder, recursive=True, verbose=True):
    folder = Path(folder)
    sv = _schema_validator()
    files = sorted(folder.rglob("*.json") if recursive else folder.glob("*.json"))
    report, bad = [], 0
    docs = {}
    for p in files:
        try:
            doc = json.loads(p.read_text(encoding="utf-8"))
        except Exception as e:          # noqa: BLE001
            report.append((p, "UNREADABLE", str(e))); bad += 1; continue
        if not isinstance(doc, dict):
            continue
        fmt = doc.get("format")
        if fmt not in SCHEMA_FOR or "schemas" in p.relative_to(folder).parts:
            report.append((p, "skipped", f"format {fmt!r} not validated by this tool")); continue
        docs[p] = doc
        msgs = []
        if sv is not None:
            errs = sv(doc)
            if errs:
                msgs.append(f"schema: {len(errs)} errors, first: {list(errs[0].absolute_path)} {errs[0].message[:160]}")
        if fmt in ("spyder-bone/model", "spyder-bone/transfer"):
            n, nb, fails, reason = SR.validate_file(p)
            msgs.append(reason or f"goldens {n} checks, {nb} failed")
            if reason or nb:
                bad += 1
            if not reason:
                pr = SR.v1_problems(SR.load_json(p))
                if pr:
                    msgs.append("v1 op subset: " + "; ".join(pr))
        elif fmt in ("spyder-bone/bands", "spyder-bone/noise_gains", "spyder-bone/instrument"):
            n, nb, fails, reason = TB.validate_table(p)
            msgs.append(reason or f"goldens {n} checks, {nb} failed")
            if reason or nb:
                bad += 1
        elif fmt in ("spyder-bone/engine_check", "spyder-bone/analysis_profile"):
            # goldens of the engine-check parameter files and analysis profiles: the oracle's runner (reference/oracle)
            try:
                sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
                from oracle import checkgold
                from oracle.params import PluginSet
                import numpy as np
                ps = PluginSet([folder])
                sp = {}
                gf = doc.get("golden", {}).get("spectra_file")
                if gf:
                    for s_ in SR.load_json(p.parent / gf)["spectra"]:
                        sp[s_["sha256"]] = (s_["wl_start_nm"] + s_["wl_step_nm"] * np.arange(len(s_["values"])),
                                            np.asarray(s_["values"], float))
                r = checkgold.run(doc, ps, sp)
                nb = sum(not x[-1] for x in r)
                msgs.append(f"goldens {len(r)} checks, {nb} failed")
                bad += 1 if nb else 0
            except Exception as e:      # noqa: BLE001  (GoldenError, ImportError, ParamError)
                msgs.append(f"REJECTED {type(e).__name__}: {e}"); bad += 1
        elif fmt == "spyder-bone/reference_set":
            import numpy as np
            n = doc["grid"]["n"]
            okr = all(len(l["absorbance"]) == n and np.all(np.isfinite(l["absorbance"])) and len(l["members"]) == l["n"]
                      for l in doc["levels"])
            msgs.append("levels OK" if okr else "REJECTED: level length, finiteness or member count"); bad += 0 if okr else 1
        elif fmt == "spyder-bone/golden_spectra":
            try:
                SR.load_golden_spectra(p); msgs.append("hashes OK")
            except SR.SpyderError as e:
                msgs.append(f"REJECTED {e}"); bad += 1
        if sv is not None and msgs and msgs[0].startswith("schema:"):
            bad += 1
        report.append((p, "checked", "; ".join(msgs)))
    # cross-file: consensus components and catalog hashes
    by_sha = {SR.file_sha256(p): p for p in docs}
    for p, doc in docs.items():
        if doc.get("format") == "spyder-bone/model" and doc.get("kind") == "consensus":
            for k in doc["consensus"]["components"]:
                q = by_sha.get(k["sha256"])
                ok = q is not None and docs[q].get("id") == k["id"] and docs[q].get("version") == k["version"]
                report.append((p, "component", f"{k['name']} -> {k['id']}@{k['version']}: {'OK' if ok else 'MISSING or MISMATCHED'}"))
                bad += 0 if ok else 1
        if doc.get("format") == "spyder-bone/catalog":
            for e in doc["entries"]:
                q = p.parent / e["file"]
                ok = q.is_file() and SR.file_sha256(q) == e["sha256"]
                if not ok:
                    report.append((p, "catalog", f"{e['file']}: sha256 MISMATCH or missing")); bad += 1
            report.append((p, "catalog", f"{len(doc['entries'])} entries checked"))
    if verbose:
        if sv is None:
            print("NOTE: jsonschema not installed; schema validation skipped (pip install jsonschema)")
        for p, st, m in report:
            print(f"{p.relative_to(folder)}: {st}: {m}")
        print(f"{'FAILED' if bad else 'OK'}: {bad} problem(s)")
    return bad, report


if __name__ == "__main__":
    b, _ = validate_folder(sys.argv[1] if len(sys.argv) > 1 else "plugins")
    sys.exit(1 if b else 0)
