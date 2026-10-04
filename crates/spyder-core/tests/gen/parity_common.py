"""Shared helpers for the Rust-vs-Python parity fixtures (PLAN Phase 2 gate: fuzz on 200 perturbed + the real
spectra, predictions within 1e-9).

* `SplitMix64` and `perturb()`: seeded perturbations of a spectrum made of basic IEEE operations only (no
  transcendental functions), so that the Rust test (tests/fuzz_parity.rs) regenerates them BIT FOR BIT; each case
  also records the spectrum's content hash, which the Rust test checks first.
* `Engine`: the plug-in folder loaded with the frozen Python reference `reference/spyder_ref` (numpy only), and
  `predict_all()`: every active model on one scan for one instrument class, with the transfer chosen by
  spyder_ref.pick_transfer (the PLAN Step 4 rule).
"""
from __future__ import annotations

import sys
from pathlib import Path

import numpy as np

sys.dont_write_bytecode = True
REPO = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO / "reference"))
import spyder_ref as SR  # noqa: E402

MASK64 = (1 << 64) - 1
STD, HIRES = "asd.labspec4.std", "asd.labspec4.hires"


class SplitMix64:
    def __init__(self, seed: int):
        self.s = seed & MASK64

    def next_u64(self) -> int:
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK64
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
        return z ^ (z >> 31)

    def uniform(self) -> float:
        """[0, 1) with 53 random bits: exact in both languages."""
        return (self.next_u64() >> 11) * (1.0 / 9007199254740992.0)


def perturb(wl, x, k: int):
    """Case k: scale, offset, detector steps (at 1000 nm and at a random upper join 1800-1830 nm), tilt and uniform
    noise; every 10th case (k % 10 == 7) also drives the first 40 channels below zero (absorbance clips).
    Returns (values, joins). The operation order is the contract with tests/fuzz_parity.rs."""
    g = SplitMix64(0x5350594445520000 + k)
    scale = 0.6 + 0.8 * g.uniform()
    offset = (g.uniform() - 0.5) * 0.04
    step1 = (g.uniform() - 0.5) * 0.04
    step2 = (g.uniform() - 0.5) * 0.04
    tilt = (g.uniform() - 0.5) * 2e-5
    amp = 1e-4 * (1.0 + 9.0 * g.uniform())
    j2 = 1800.0 + 10.0 * float(g.next_u64() >> 62)
    out = []
    for i in range(len(x)):
        w = float(wl[i])
        v = float(x[i]) * scale + offset
        if w <= 1000.0:
            v = v + step1
        elif w > j2:
            v = v + step2
        v = v + tilt * (w - 1500.0)
        v = v + amp * (2.0 * g.uniform() - 1.0)
        if k % 10 == 7 and i < 40:
            v = -0.01
        out.append(v)
    return np.array(out, dtype=float), [1000.0, j2]


class Engine:
    """The plug-in folder as spyder_ref sees it: active models and active transfers."""

    def __init__(self, folder: Path):
        self.folder = Path(folder)
        self.models, self.transfers, self.files = [], [], {}
        for p in sorted(self.folder.glob("*.spyder-model.json")):
            d = SR.load_json(p)
            self.files[p.name] = SR.file_sha256(p)
            if d.get("status") == "active":
                SR.check_model(d)
                d["_sha256"] = self.files[p.name]
                self.models.append(d)
        for p in sorted(self.folder.glob("*.spyder-transfer.json")):
            t = SR.check_transfer(SR.load_json(p))
            t["_sha256"] = SR.file_sha256(p)
            self.files[p.name] = t["_sha256"]
            self.transfers.append(t)
        self.models.sort(key=lambda m: m["id"])

    def predict_all(self, wl, x, joins, cls, serial=None):
        """{model id: outputs or {"error": ...}} for one scan and class."""
        ctx = SR.ScanContext(tuple(joins), cls, serial)
        X = np.asarray(x, float)[None, :]
        out = {}
        cache = {}
        for m in self.models:
            rec = {}
            try:
                t = SR.pick_transfer(m, ctx, self.transfers)
                if isinstance(t, dict):
                    rec["transfer"] = t["_sha256"]
                    if t["_sha256"] not in cache:
                        cache[t["_sha256"]] = SR.apply_transfer(t, wl, X, ctx)
                    wl2, X2 = cache[t["_sha256"]]
                else:
                    rec["transfer"] = None
                    wl2, X2 = wl, X
                p = SR.predict(m, wl2, X2, ctx)
                for k, v in p.items():
                    if k in ("value", "linear", "T2", "Q", "domain_ratio") or k.startswith("component:"):
                        rec[k] = float(np.asarray(v).ravel()[0])
            except SR.SpyderError as e:
                rec = {"error": str(e)}
            out[m["id"]] = rec
        return out
