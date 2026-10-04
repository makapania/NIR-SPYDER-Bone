"""Synthetic golden spectra for rule branches (golden generation only; not used when scoring).

Every feature the rule branches depend on (model outputs, band readings, sign statistics, C1 excess) is LINEAR in
absorbance A = log10(1/R) as long as no clip is reached. So a public base spectrum (a JAS mmc2 row) can be moved to
hit chosen feature values exactly: find the smallest smooth absorbance perturbation dA = B c (B: Gaussian bumps,
sigma `bump_sigma_nm`, every `bump_step_nm` over `range_nm`) with J B c = target - f(A0), where J is the feature
Jacobian (exact by linearity: one evaluation per bump). Features without a target are held at their base values.
Optional seeded white noise in A (per window) sets the canonical N2 for noise branches; it is added first.

A recipe (in reference/oracle/goldens/cases_public_v1.json):
  {"base": "<spectrum sha256>", "noise": [{"window_nm": [lo, hi], "n2": 62.5, "seed": 1}],
   "targets": {"m.wc2045": 4.0, "E.AM2175": -0.4, ...}, "hold": [...feature names...],
   "scale_nm": [[lo, hi, factor], ...]   (multiply R in a range: VNIR darkening for the burnt branch),
   "bumps": {"range_nm": [1100, 2440], "sigma_nm": 4.0, "step_nm": 4}}
Feature names: m.<component> (CONS3 component), m.ryder, E.<band id>, wax1766, wax1217, ester2254, plaster1446,
plaster1945, c1_excess.
"""
from __future__ import annotations

import numpy as np

from . import kernels as K
from ._ref import SR

WL = np.arange(350, 2501).astype(float)


class FeatureMap:
    def __init__(self, oracle):
        self.o = oracle
        P = oracle.P
        rc = P.profile("radiocarbon").doc["parameters"]
        self.cons = P.model([s for s in rc["models"] if s["role"] == "verdict_input"][0]["model_id"]).doc
        self.ryder = P.model([s for s in rc["models"] if s["role"] == "published"][0]["model_id"]).doc
        self.bands = P.bands()
        self.ev_k = P.cp("evidence_levels")["kernel"]
        self.sg = P.cp("signs")
        self.c1 = P.cp("c1")
        self.ctx = SR.ScanContext()

    def comps(self):
        if self.cons.get("kind") == "consensus":
            return [(c["name"], f"component:{c['name']}") for c in self.cons["consensus"]["components"]]
        return [(k, f"feature:{k}") for k in self.cons["rule"]["features"]]

    def c1_only_bands(self):
        ev, zo = self.o.P.cp("evidence_levels"), self.o.P.cp("zooms_patterns")
        used = set(ev["core_bands"]) | {ev["extra_band"]} | set(zo["protein_bands"]) | set(zo["ch_bands"])
        return [b for b in self.bands if b not in used]

    def names(self):
        comps = [k for k, _ in self.comps()]
        return ([f"m.{c}" for c in comps] + ["m.ryder"] + [f"E.{b}" for b in self.bands]
                + ["wax1766", "wax1217", "ester2254", "plaster1446", "plaster1945", "c1_excess"])

    def __call__(self, R):
        R = np.atleast_2d(R)
        out = {}
        pc = SR.predict(self.cons, WL, R, self.ctx)
        for k, key in self.comps():
            out[f"m.{k}"] = pc[key]
        out["m.ryder"] = SR.predict(self.ryder, WL, R, self.ctx)["value"]
        E = K.derivative(R, self.ev_k)
        for b, bd in self.bands.items():
            out[f"E.{b}"] = K.band_value(E, WL, bd)
        N = K.derivative(R, self.sg["kernel"])
        for t in self.sg["wax"]["all_of"]:
            out[f"wax{t['sharp'][0]}"] = K.sharp(N, WL, *t["sharp"])
        out["ester2254"] = K.sharp(N, WL, *self.sg["ester"]["sharp"])
        for b in self.sg["plaster"]["bands"]:
            out[f"plaster{b['sharp'][0]}"] = K.sharp(N, WL, *b["sharp"])
        hw = self.c1["read_half_width_nm"]
        ch = sum(K.band_reading(E, WL, c, hw) for c in self.c1["ch_bands_nm"])
        nh = sum(K.band_reading(E, WL, c, hw) for c in self.c1["nh_bands_nm"])
        out["c1_excess"] = ch - (self.c1["a"] + self.c1["b"] * nh)
        return {k: np.asarray(v, float) for k, v in out.items()}


def add_noise(A, spec):
    """Seeded white noise in A within each window, scaled so that the canonical N2 there hits the target."""
    from .n2 import n2 as N2
    A = A.copy()
    for s in spec:
        lo, hi = s["window_nm"]
        k = (WL >= lo - 1) & (WL <= hi)
        rng = np.random.default_rng(int(s["seed"]))
        z = rng.standard_normal(int(k.sum()))
        base = N2(10.0 ** -A, lo, hi)
        lo_s, hi_s = 0.0, 1e-2
        for _ in range(80):                        # bisection on the noise scale (N2 is monotone in it)
            mid = 0.5 * (lo_s + hi_s)
            B = A.copy(); B[k] += mid * z
            if N2(10.0 ** -B, lo, hi) < s["n2"]:
                lo_s = mid
            else:
                hi_s = mid
        A[k] += hi_s * z
        _ = base
    return A


def synthesise(oracle, R0, recipe):
    fm = FeatureMap(oracle)
    A = np.log10(1.0 / np.maximum(np.asarray(R0, float), 1e-6))
    for lo, hi, fac in recipe.get("scale_nm", []):
        k = (WL >= lo) & (WL <= hi)
        A[k] -= np.log10(fac)
    if recipe.get("noise"):
        A = add_noise(A, recipe["noise"])
    for lo, hi, val in recipe.get("set_nm", []):
        k = (WL >= lo) & (WL <= hi)
        A[k] = -np.log10(val) if val > 0 else A[k]
    tg = recipe.get("targets", {})
    if tg:
        bp = recipe.get("bumps", {"ranges_nm": [[1100, 2440]], "sigma_nm": 6.0, "step_nm": 3})
        cents = np.concatenate([np.arange(lo, hi + 1e-9, bp["step_nm"]) for lo, hi in bp["ranges_nm"]])
        Bm = np.exp(-0.5 * ((WL[:, None] - cents[None, :]) / bp["sigma_nm"]) ** 2)       # (2151, nb)
        # C1's own bands (no role in the evidence or ZooMS rules) are free by default: c1_excess is a linear
        # combination of band readings, so holding all of them AND c1_excess would be contradictory
        free = set(recipe.get("free", [])) | ({f"E.{b}" for b in fm.c1_only_bands()} - set(tg))
        names = [n for n in fm.names() if n not in free]
        for n in tg:
            if n not in names:
                raise ValueError(f"unknown synth target {n}")
        f0 = fm(10.0 ** -A)
        J = np.zeros((len(names), Bm.shape[1]))
        for j in range(Bm.shape[1]):
            fj = fm(10.0 ** -(A + Bm[:, j]))
            for i, n in enumerate(names):
                J[i, j] = fj[n][0] - f0[n][0]
        rhs = np.array([tg[n] - f0[n][0] if n in tg else 0.0 for n in names])
        # smallest CURVATURE perturbation that hits the targets: minimise ||second difference of B c||^2 (+ a tiny
        # ridge) subject to J c = rhs; keeps the canonical N2 (a second-difference statistic) near the base value
        D2 = Bm[2:] - 2 * Bm[1:-1] + Bm[:-2]
        H = D2.T @ D2
        H += 1e-9 * np.trace(H) / H.shape[0] * np.eye(H.shape[0])
        Hi_Jt = np.linalg.solve(H, J.T)
        lam = np.linalg.lstsq(J @ Hi_Jt, rhs, rcond=None)[0]
        c = Hi_Jt @ lam
        A = A + Bm @ c
    R = 10.0 ** -A
    for lo, hi, val in recipe.get("set_nm", []):
        if val <= 0:
            R[(WL >= lo) & (WL <= hi)] = val
    return R


def generate(gen):
    """Model-free synthetic spectra (non-bone targets, panels)."""
    k = gen["kind"]
    if k == "flat":
        R = np.full(WL.size, float(gen["level"]))
        if gen.get("noise_sd"):
            R = R + np.random.default_rng(int(gen.get("seed", 0))).normal(0, gen["noise_sd"], WL.size)
        return R
    if k == "sine":
        return gen["mean"] + gen["amp"] * np.sin(2 * np.pi * (WL - 350) / gen["period_nm"])
    raise ValueError(f"unknown generator {k}")
