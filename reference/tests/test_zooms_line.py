"""ZooMS line (DECISIONS 80 amended) regressions from the Codex review of the build, on the frozen oracle. The same
rejection runs in Rust (crates/spyder-core/tests/plugins_rules.rs). Public data only."""
from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

import pytest

REF = Path(__file__).resolve().parents[1]
PLUG = REF.parent / "plugins"
sys.path.insert(0, str(REF))
from oracle import checkgold  # noqa: E402

PROFILE = json.loads((PLUG / "profiles" / "profile.radiocarbon.json").read_text(encoding="utf-8"))
FAINT_INPUTS = {"m": 0.2, "components": {"wc2045": 0.2, "wc1500": 0.2, "F05": 0.2}, "evidence_level": "trace",
                "zooms_verdict": "Borderline", "zooms_pattern": "C", "signs_fired": [], "c1_status": "assessed",
                "gated": False, "n_type_lit": ["AM2175"]}


def test_empty_protein_band_list_does_not_enable_strong_line():
    # the shipped profile: m 0.2 with 2175 alone is the faint sign, never the protein line
    assert checkgold.compute_profile(PROFILE, FAINT_INPUTS)["notes_all"] == "zooms_faint_protein"
    # an empty band list is an error in the engine (the Rust loader rejects the file; the schema has minItems 1)
    bad = copy.deepcopy(PROFILE)
    bad["parameters"]["notes"]["zooms_better"]["protein"]["n_type_bands"] = []
    with pytest.raises(ValueError, match="n_type_bands"):
        checkgold.compute_profile(bad, FAINT_INPUTS)
    schema = json.loads((PLUG / "schemas" / "analysis_profile.schema.json").read_text(encoding="utf-8"))
    coll = [s for s in schema["properties"]["parameters"]["oneOf"] if s["properties"]["kind"].get("const") == "collagen_rule_L2"][0]
    prot = coll["properties"]["notes"]["properties"]["zooms_better"]["properties"]["protein"]
    assert prot["properties"]["n_type_bands"]["minItems"] == 1


def test_unreadable_lit_vote_band_does_not_count():
    # the production shape of the ZooMS check: a lit but unreadable 1545 vote band is not an N-type band for the test
    zc = {"bands": {"NH2044": {"lit": False, "readable": True}, "AM2175": {"lit": True, "readable": True}},
          "vote_band": {"id": "NH1545c", "lit": True, "readable": False}}
    inp = dict(FAINT_INPUTS, m=0.4, zooms_check=zc)
    inp.pop("n_type_lit")
    assert checkgold.compute_profile(PROFILE, inp)["notes_all"] == "zooms_faint_protein"
    zc["vote_band"]["readable"] = True
    assert checkgold.compute_profile(PROFILE, inp)["notes_all"] == "zooms_better_protein"
