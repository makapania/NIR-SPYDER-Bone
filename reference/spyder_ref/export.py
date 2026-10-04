"""Exporter: write a linear collagen model as a SPYDER Bone model file with goldens from the research path.

Developer tool (needs scipy for the golden generator). Steps, each a hard check (PLAN section 4 "Adding a model"):
  1. build the file body (chain, explicit feature wavelengths, centred linear form y = offset + sum b_i (x_i - c_i));
  2. structural check (core.check_model) and the v1 op subset (core.v1_problems);
  3. golden cases from spyder_ref.golden_gen (scipy path, independent of the engine), >= 3 cases, one with features;
  4. write JSON with allow_nan=False;
  5. run the goldens with the engine (core.validate_file) at the engine tolerance (1e-9).

    from spyder_ref.export import export_regression
    export_regression("out/my.spyder-model.json", id="collagen.lab.my_model", version="1.0.0", title=..., chain=[...],
                      wavelengths_nm=[...], coefficients=[...], x_center=[...], offset=..., golden_spectra_file=path, ...)
The golden spectra file must sit in the same folder as the model file (plug-ins are path-confined).
"""
from __future__ import annotations
import json
from datetime import date
from pathlib import Path
from . import core as SR
from . import golden_gen as GG


def export_regression(path, *, id, version, title, chain, wavelengths_nm, coefficients, offset, x_center=None,
                      golden_spectra_file, golden_names=None, status="experimental", engine_min="1.0", short_name=None,
                      description="", citation=None, provenance=None, instrument=None, error=None, statistics=None,
                      licence="CC-BY-4.0", role="collagen_percent"):
    path = Path(path)
    gs_path = Path(golden_spectra_file)
    if gs_path.parent.resolve() != path.parent.resolve():
        raise SR.SpyderError("the golden spectra file must be in the model's own folder")
    doc = {"format": SR.MODEL_FORMAT, "format_version": SR.FORMAT_VERSION, "engine_min": engine_min, "id": id, "version": version,
           "status": status, "title": title, "short_name": short_name or title, "description": description, "kind": "regression",
           "role": role, "citation": citation or {"text": ""}, "authors": [], "created": str(date.today()),
           "exported": str(date.today()), "licence": licence, "provenance": provenance or {},
           "instrument": instrument or {"trained_on_class": "asd.labspec4.std", "grid": {"start_nm": 350.0, "step_nm": 1.0, "n": 2151},
                                        "training_splices_nm": [1000.0, 1800.0]},
           "input": {"quantity": "reflectance", "scale": "fraction", "range_nm": [350, 2500]},
           "preprocessing": list(chain), "features": {"wavelengths_nm": [float(w) for w in wavelengths_nm]},
           "regression": {"coefficients": [float(b) for b in coefficients], "offset": float(offset)},
           "output": {"name": "collagen", "units": "wt% collagen", "y_transform": {"type": "none"}, "decimals": 1},
           "error": error or {"display": "none"}, "domain": {"method": "none"}}
    if x_center is not None:
        doc["regression"]["x_center"] = [float(c) for c in x_center]
    if statistics:
        doc["statistics"] = statistics
    doc["golden"] = {"spectra_file": Path(golden_spectra_file).name, "tolerance": {"abs": 1e-9, "rel": 1e-9}, "cases": []}
    SR.check_model(doc)
    pr = SR.v1_problems(doc)
    if pr:
        raise SR.SpyderError("not in the v1 op subset: " + "; ".join(pr))
    gs = json.loads(gs_path.read_text(encoding="utf-8"))
    spectra = [s for s in gs["spectra"] if golden_names is None or s["name"] in golden_names]
    if len(spectra) < SR.GOLDEN_MIN_CASES:
        raise SR.SpyderError("need at least 3 golden spectra")
    doc["golden"] = {"spectra_file": gs_path.name, "tolerance": {"abs": 1e-9, "rel": 1e-9}, "cases": GG.model_cases(doc, spectra)}
    path.write_text(json.dumps(doc, indent=1, allow_nan=False) + "\n", encoding="utf-8")
    n, nb, bad, reason = SR.validate_file(path)
    if reason or nb:
        raise SR.SpyderError(f"exported file fails its goldens: {reason or bad[:3]}")
    return n
