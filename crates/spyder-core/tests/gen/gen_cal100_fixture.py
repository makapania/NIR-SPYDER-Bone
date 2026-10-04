"""Write the PUBLIC fixture for the Cal-100 Table 5 reproduction test (tests/cal100.rs; PLAN Phase 2 gate):

  tests/fixtures/cal100/test_ryder2045_cal100.spyder-model.json
      the Calibration-100 reproduction model (test only, never shipped; status demo), with provenance text
      anonymised and its goldens pointed at plugins/golden_spectra_public_v2.json (same spectrum hashes)
  tests/fixtures/cal100/mmc2_2045_window.json
      reflectance 1990-2100 nm (the 2030-2060 nm features + the SG31 support + margin) of the 100 Calibration and
      40 Validation reference bones, their collagen yields and set labels, and the published Table 5 values

Sources (public): Ryder et al. 2026, J. Archaeol. Sci. 185:106448, supplement mmc2.xlsx (spectra and yields;
DECISIONS 31: free to share), and the Calibration / Validation labels of the public GitHub repository
gerlis22/NIR_Collagen ('Manuscript Transformed Data_Revised.xlsx').

    set SPYDER_MMC2_XLSX=<path to mmc2.xlsx>            (default: <repo>/Papers/sciencedirect_supp/...-mmc2.xlsx)
    set SPYDER_NIR_COLLAGEN_XLSX=<path to the GitHub workbook>
    python crates/spyder-core/tests/gen/gen_cal100_fixture.py
"""
import json
import os
import sys
from pathlib import Path

import numpy as np
import pandas as pd

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
from parity_common import REPO, SR  # noqa: E402

OUT = REPO / "crates" / "spyder-core" / "tests" / "fixtures" / "cal100"
MMC2 = Path(os.environ.get("SPYDER_MMC2_XLSX",
                           REPO / "Papers" / "sciencedirect_supp" / "1-s2.0-S0305440325002973-mmc2.xlsx"))
GH = Path(os.environ["SPYDER_NIR_COLLAGEN_XLSX"])
MODEL_SRC = Path(os.environ.get("SPYDER_CAL100_MODEL",
                                REPO / "planning" / "work" / "bundles" / "public" / "test_ryder2045_cal100.spyder-model.json"))
LO, HI = 1990, 2100
TABLE5 = {"R2C": 0.8878, "RMSEC": 1.591, "VR2_val40": 0.8851, "RMSEV_val40": 1.6164,
          "val40_below3_correct": "20/24", "val40_above3_correct": "14/16",
          "decimals": {"R2C": 4, "RMSEC": 3, "VR2_val40": 4, "RMSEV_val40": 4}}


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    s = pd.read_excel(MMC2)
    s.columns = ["sample"] + list(s.columns[1:])
    wl = np.array(s.columns[4:], float)
    assert np.array_equal(wl, np.arange(350, 2501))
    s["sample"] = s["sample"].astype(str).str.strip()
    R = s.iloc[:, 4:].to_numpy(float)
    y = s["Collagen Yield (%)"].to_numpy(float)
    gh = pd.read_excel(GH)
    gh.columns = ["sample", "set"] + list(gh.columns[2:])
    gh["sample"] = gh["sample"].astype(str).str.strip()
    assert list(gh["sample"]) == list(s["sample"])
    sets = gh["set"].to_numpy().astype(str)
    keep = (sets == "Calibration") | (sets == "Validation")
    assert (sets == "Calibration").sum() == 100 and (sets == "Validation").sum() == 40
    win = (wl >= LO) & (wl <= HI)
    bones = [{"name": str(s["sample"][i]), "set": sets[i], "collagen_pct": float(y[i]),
              "reflectance": [float(v) for v in R[i, win]]} for i in np.flatnonzero(keep)]
    (OUT / "mmc2_2045_window.json").write_text(json.dumps({
        "format": "spyder-bone/test-fixture",
        "source": "Ryder et al. 2026, J. Archaeol. Sci. 185:106448, supplement mmc2 (public); Calibration/Validation "
                  "labels from the public GitHub repository gerlis22/NIR_Collagen",
        "mmc2_sha256": SR.file_sha256(MMC2),
        "wl_start_nm": float(LO), "wl_step_nm": 1.0,
        "note": "reflectance over 1990-2100 nm only: the model's 2030-2060 nm features need 2015-2075 nm (SG31)",
        "table5_published": TABLE5,
        "bones": bones}, allow_nan=False), encoding="utf-8")
    m = json.loads(MODEL_SRC.read_text(encoding="utf-8"))
    m["golden"]["spectra_file"] = "golden_spectra_public_v2.json"
    gs = SR.load_golden_spectra(REPO / "plugins" / "golden_spectra_public_v2.json")
    assert all(c["spectrum_sha256"] in gs for c in m["golden"]["cases"])
    m["provenance"] = {
        "training_data": m["provenance"]["training_data"],
        "training_data_sha256": m["provenance"]["training_data_sha256"],
        "split_labels_sha256": m["provenance"]["split_labels_sha256"],
        "model_algorithm": m["provenance"]["model_algorithm"],
        "latent_variables": m["provenance"]["latent_variables"],
        "n_train": m["provenance"]["n_train"],
        "golden_generated_by": "research path: an independent re-implementation of the Ryder/Unscrambler transform "
                               "and NIPALS PLS1 (private research tree)",
        "environment": m["provenance"]["environment"],
        "note": "test fixture: provenance anonymised and goldens pointed at golden_spectra_public_v2.json by "
                "crates/spyder-core/tests/gen/gen_cal100_fixture.py; coefficients and goldens unchanged",
    }
    if isinstance(m.get("reproduction"), dict):
        m["reproduction"].pop("test", None)
    text = json.dumps(m, indent=1, allow_nan=False) + "\n"
    for bad in ("Border", "P:/", "planning/"):
        assert bad not in text, bad
    (OUT / "test_ryder2045_cal100.spyder-model.json").write_text(text, encoding="utf-8")
    # the reference engine must still pass the fixture's goldens
    import shutil
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        shutil.copy(OUT / "test_ryder2045_cal100.spyder-model.json", td)
        shutil.copy(REPO / "plugins" / "golden_spectra_public_v2.json", td)
        n, nb, _, reason = SR.validate_file(Path(td) / "test_ryder2045_cal100.spyder-model.json")
        assert reason is None and nb == 0, (n, nb, reason)
    print(f"wrote {OUT.relative_to(REPO)}: {len(bones)} bones; model goldens {n} checks pass in spyder_ref")


if __name__ == "__main__":
    main()
