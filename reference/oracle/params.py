"""Load every parameter the oracle uses from plug-in files. Nothing numeric that lives in a shipped file is written
in the oracle's code.

A PluginSet is built from one or more root folders, searched recursively for *.json (and sidecars). For the same
(format, id) the FIRST root that holds it wins (so the shipped `plugins/` can be listed before a staging folder);
inside one root the highest version wins. Golden-spectra files are indexed separately.

Kinds read here:
  spyder-bone/engine_check      check parameter files (this package's own format; schemas in plugins/schemas/)
  spyder-bone/analysis_profile  radiocarbon / isotopes / zooms profiles
  spyder-bone/model, /transfer  through spyder_ref
  bands, noise gains, instrument registry: read through small adapters (_norm_*), because those files are owned by
  another work package; each adapter turns the file into the normalised form documented on the adapter.
"""
from __future__ import annotations

import hashlib
import json
from pathlib import Path

import numpy as np

from ._ref import SR

CHECK_FORMAT = "spyder-bone/engine_check"
PROFILE_FORMAT = "spyder-bone/analysis_profile"
GOLDEN_SPECTRA_FORMAT = "spyder-bone/golden_spectra"


class ParamError(ValueError):
    pass


def _ver(v):
    out = []
    for p in str(v).split("-")[0].split("."):
        try:
            out.append(int(p))
        except ValueError:
            out.append(0)
    return tuple(out)


def sha256_file(p) -> str:
    return hashlib.sha256(Path(p).read_bytes()).hexdigest()


class Doc:
    def __init__(self, path, doc, root_rank):
        self.path = Path(path)
        self.doc = doc
        self.sha256 = sha256_file(path)
        self.root_rank = root_rank

    @property
    def id(self):
        return self.doc.get("id")

    @property
    def version(self):
        return str(self.doc.get("version"))

    @property
    def ref(self):
        return f"{self.id}@{self.version}"


class PluginSet:
    def __init__(self, roots):
        self.roots = [Path(r) for r in roots if r and Path(r).exists()]
        if not self.roots:
            raise ParamError("no plug-in root exists")
        self.docs: list[Doc] = []
        self.spectra_files: dict[str, Path] = {}
        self.sidecars: dict[str, Path] = {}
        for rank, root in enumerate(self.roots):
            for p in sorted(root.rglob("*")):
                if p.is_dir() or "__pycache__" in p.parts:
                    continue
                if p.suffix.lower() == ".npy":
                    self.sidecars.setdefault(p.name, p)
                    continue
                if p.suffix.lower() != ".json" or "schema" in p.name.lower():
                    continue
                try:
                    d = json.loads(p.read_text(encoding="utf-8"))
                except (json.JSONDecodeError, UnicodeDecodeError):
                    continue
                if not isinstance(d, dict) or "format" not in d:
                    continue
                if str(d["format"]).startswith(GOLDEN_SPECTRA_FORMAT):
                    self.spectra_files.setdefault(p.name, p)
                    continue
                self.docs.append(Doc(p, d, rank))
        self._cache = {}

    # ------------------------------------------------------------------ generic lookup
    def find(self, fmt_prefix, pred=lambda d: True, required=True, what=""):
        c = [d for d in self.docs if str(d.doc.get("format", "")).startswith(fmt_prefix) and pred(d.doc)
             and d.doc.get("status", "active") != "withdrawn"]
        if not c:
            if required:
                raise ParamError(f"no {what or fmt_prefix} file found in {', '.join(map(str, self.roots))}")
            return None
        best_rank = min(d.root_rank for d in c)
        c = [d for d in c if d.root_rank == best_rank]
        return max(c, key=lambda d: _ver(d.version))

    def check(self, name) -> Doc:
        k = ("check", name)
        if k not in self._cache:
            self._cache[k] = self.find(CHECK_FORMAT, lambda d: d.get("check") == name, what=f"engine check '{name}'")
        return self._cache[k]

    def cp(self, name) -> dict:
        """parameters block of a check file"""
        return self.check(name).doc["parameters"]

    def profile(self, analysis) -> Doc:
        k = ("profile", analysis)
        if k not in self._cache:
            self._cache[k] = self.find(PROFILE_FORMAT, lambda d: d.get("analysis") == analysis,
                                       what=f"analysis profile '{analysis}'")
        return self._cache[k]

    def model(self, mid) -> Doc:
        k = ("model", mid)
        if k not in self._cache:
            self._cache[k] = self.find("spyder-bone/model", lambda d: d.get("id") == mid, what=f"model '{mid}'")
        return self._cache[k]

    def transfers(self) -> list[Doc]:
        out = {}
        for d in self.docs:
            if str(d.doc.get("format")) == "spyder-bone/transfer" and d.doc.get("status") != "withdrawn":
                key = d.id
                if key not in out or (d.root_rank, tuple(-x for x in _ver(d.version))) < (
                        out[key].root_rank, tuple(-x for x in _ver(out[key].version))):
                    out[key] = d
        return list(out.values())

    def transfer_doc_for(self, model_doc, ctx):
        """spyder_ref.pick_transfer over the available transfers; returns ('none'|Doc|None)."""
        ts = self.transfers()
        pick = SR.pick_transfer(model_doc, ctx, [t.doc for t in ts])
        if pick is None or pick == "none":
            return pick
        for t in ts:
            if t.doc is pick:
                return t
        raise ParamError("transfer pick not found")

    def stream_transfer(self, instrument_class, serial, target_class="asd.labspec4.std"):
        """The transfer that makes the standard-resolution-equivalent stream (Step 4) for this class."""
        fake = {"instrument": {"trained_on_class": target_class}}
        ctx = SR.ScanContext((1000.0, 1800.0), instrument_class, serial)
        return self.transfer_doc_for(fake, ctx)

    def sidecar(self, name, sha):
        p = self.sidecars.get(name)
        if p is None:
            raise ParamError(f"sidecar {name} not found in the plug-in roots")
        if sha256_file(p) != sha:
            raise ParamError(f"sidecar {name}: file sha256 mismatch")
        return p

    # ------------------------------------------------------------------ adapters for files owned elsewhere
    def bands(self) -> dict:
        if "bands" not in self._cache:
            d = self.find("spyder-bone/bands", what="bands table (bands.json)")
            self._cache["bands"] = (_norm_bands(d.doc), d)
        return self._cache["bands"][0]

    def band_gain(self, band_id, tkey):
        b = self.bands().get(band_id, {})
        g = (b.get("gain_E_per_N2") or {}).get(tkey)
        return float(g) if g is not None else self.gains().band_gain(band_id, tkey)

    def kernels(self) -> dict:
        """bands.json kernels, normalised to the check files' key names."""
        self.bands()
        k = self._cache["bands"][1].doc.get("kernels", {})
        out = {}
        for name, v in k.items():
            out[name] = {"absorbance_clip": v.get("absorbance_clip", v.get("absorbance_clip_min")),
                         "savgol": {kk: v["savgol"][kk] for kk in ("window", "polyorder", "deriv", "delta")},
                         "scale": v.get("scale")}
        return out

    def gains(self) -> "Gains":
        if "gains" not in self._cache:
            d = self.find("spyder-bone/noise_gains", what="noise gains (noise_gains.json)")
            self._cache["gains"] = (Gains(d.doc, d), d)
        return self._cache["gains"][0]

    def registry(self) -> dict:
        if "registry" not in self._cache:
            d = self.find("spyder-bone/instrument", required=False)
            self._cache["registry"] = (_norm_registry(d.doc) if d else {}, d)
        return self._cache["registry"][0]

    # ------------------------------------------------------------------ provenance
    def used(self):
        """id@version -> sha256 of every file read so far (for the analysis manifest)."""
        out = {}
        for k, v in self._cache.items():
            docs = v if isinstance(v, tuple) else (v,)
            for d in docs:
                if isinstance(d, Doc):
                    out[d.ref] = d.sha256
        return dict(sorted(out.items()))


# ---------------------------------------------------------------------- adapters
def _norm_bands(doc):
    """Normalised band table: {band_id: {centre_nm, half_width_nm, weight, faint_u, detector, noise_window_nm,
    gain_E_per_N2 {transfer key: gain}, kernel, u_sign (+1 / -1), projection (optional: {name, window_nm, direction,
    centre, scale}: the band is read on the projected E, see kernels.band_value)}}. Readability gains live in bands.json (readability block); a
    noise_gains.json 'bands' block is the fallback.

    Accepts the staging layout {"bands": {id: {...same keys...}}} and the list layout {"bands": [{"id": ...}, ...]};
    key aliases are mapped below. Extend here when the shipped bands.json layout differs."""
    B = doc.get("bands")
    if isinstance(B, list):
        B = {b.get("id") or b.get("name"): b for b in B}
    if not isinstance(B, dict) or not B:
        raise ParamError("bands.json: no 'bands' table")
    alias = {"centre_nm": ("centre_nm", "center_nm", "centre"), "half_width_nm": ("half_width_nm", "read_half_width_nm"),
             "weight": ("weight", "weight_w", "w", "weight_w_b"), "faint_u": ("faint_u", "faint_threshold_u", "faint"),
             "detector": ("detector",), "noise_window_nm": ("noise_window_nm", "n2_window_nm")}
    out = {}
    for bid, b in B.items():
        r = {}
        for k, names in alias.items():
            for nm in names:
                if nm in b:
                    r[k] = b[nm]
                    break
        rb = b.get("readability") or {}
        if rb.get("noise_window_nm"):
            r["noise_window_nm"] = rb["noise_window_nm"]
        if rb.get("gain_E_per_N2"):
            r["gain_E_per_N2"] = dict(rb["gain_E_per_N2"])
        if b.get("kernel"):
            r["kernel"] = b["kernel"]
        if b.get("projection"):
            pj = (doc.get("projections") or {}).get(b["projection"])
            if pj is None:
                raise ParamError(f"bands.json: band {bid} names an unknown projection {b['projection']}")
            ks = (doc.get("kernels") or {}).get(pj["kernel"], {})
            r["projection"] = {"name": b["projection"], "window_nm": list(pj["window_nm"]),
                               "direction": [float(x) for x in pj["direction"]],
                               "centre": [float(x) for x in pj["centre"]], "scale": float(ks.get("scale", 1.0))}
        r["u_sign"] = int(b.get("u_sign", 1))
        if "centre_nm" not in r:
            raise ParamError(f"bands.json: band {bid} has no centre")
        r.setdefault("half_width_nm", 2)
        out[bid] = r
    return out


def _norm_registry(doc):
    """{serial(int): class} from an instrument registry. Accepts {"classes": {cls: {"serials": [...]}}} or a list of
    instrument entries [{"class": ..., "serials"| "match": {"serials": [...]}}]."""
    out = {}
    C = doc.get("classes") or doc.get("instruments")
    if isinstance(C, dict):
        for cls, v in C.items():
            for s in v.get("serials", []) or v.get("match", {}).get("serials", []):
                out[int(s)] = v.get("class", cls)
    elif isinstance(C, list):
        for v in C:
            for s in v.get("serials", []) or v.get("match", {}).get("serials", []):
                out[int(s)] = v["class"]
    elif "class" in doc:
        for s in doc.get("serials", []) or doc.get("match", {}).get("serials", []):
            out[int(s)] = doc["class"]
    return out


class Gains:
    """Noise-propagation gains keyed by model id (or band id) AND transfer key ('none' or the transfer file's sha256).

    Normalised staging layout:
      {"bands":  {band_id: {transfer_key: gain_E_per_N2}},                SD_E = gain x N2(band's noise window)
       "models": {model_id: {transfer_key: {"terms": [{"window_nm": [lo, hi], "gain": g}, ...]}}},
                                                                        SD% = sqrt(sum (g_i x N2_i)^2)
       "consensus": {model_id: {"factor": f, "components": {feature: model_key}}}}   SD = f x median(component SDs)
    """

    def __init__(self, doc, d):
        self.doc = doc
        self.file = d

    def band_gain(self, band_id, tkey):
        b = self.doc.get("bands", {}).get(band_id)
        if b is None or tkey not in b:
            return None
        return float(b[tkey])

    def model_terms(self, model_key, tkey):
        m = self.doc.get("models", {}).get(model_key)
        if m is None or tkey not in m:
            return None
        return m[tkey]["terms"]

    def consensus(self, model_id):
        return self.doc.get("consensus", {}).get(model_id)


def transfer_key(tdoc):
    """'none' for no transfer, else the transfer file's sha256 (noise gains are keyed by it; PLAN Step 2b)."""
    if tdoc is None or tdoc == "none":
        return "none"
    return tdoc.sha256


def load_npy(path):
    return np.load(path, allow_pickle=False)
