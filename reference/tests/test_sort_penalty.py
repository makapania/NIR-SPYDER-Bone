"""DECISIONS 99: within each verdict group, scans with a contaminant flag (plaster, wax, ester, C1) sort after the clean
ones; heat is not a contaminant. Expected values written from the ruling by hand (LESSONS 44), not from running the
code. The same cases run in Rust (crates/spyder-core/tests/plugins_rules.rs) and the UI (app/ui/src/lib/sort.test.ts)."""
from __future__ import annotations

import copy
import json
import sys
from pathlib import Path

import pytest

REF = Path(__file__).resolve().parents[1]
PLUG = REF.parent / "plugins"
sys.path.insert(0, str(REF))
from oracle import verdict as VD  # noqa: E402

PROF = {a: json.loads((PLUG / "profiles" / f"profile.{a}.json").read_text(encoding="utf-8"))["parameters"]
        for a in ("radiocarbon", "isotopes", "zooms")}
GOLD = {c["name"]: c["expected"] for c in
        json.loads((REF / "oracle" / "goldens" / "oracle_public_v1.json").read_text(encoding="utf-8"))["cases"]}


def flag(key, *signs):
    return {"key": key, "signs": list(signs)}


@pytest.mark.parametrize("analysis", ["radiocarbon", "isotopes"])
@pytest.mark.parametrize("flags, want", [
    ([], 0),
    ([flag("flag_contaminant", "wax")], 1),
    ([flag("flag_contaminant", "ester")], 1),
    ([flag("flag_contaminant", "plaster")], 1),
    ([flag("flag_contaminant", "C1")], 1),
    ([flag("flag_burnt", "burnt")], 0),                                    # heat only lowers readings: no penalty
    ([flag("flag_contaminant", "C1"), flag("flag_burnt", "burnt")], 1),
])
def test_penalty_from_flags(analysis, flags, want):
    assert VD.sort_penalty(PROF[analysis], {"verdict": "Unlikely", "flags": flags}) == want


def test_profile_without_penalty_writes_no_key():
    assert "penalty_signs" not in PROF["zooms"]["sort"]
    assert VD.sort_penalty(PROF["zooms"], {"flags": [flag("flag_contaminant", "wax")]}) is None


def test_penalty_sign_must_be_a_contaminant_flag_sign():
    bad = copy.deepcopy(PROF["radiocarbon"])
    bad["sort"]["penalty_signs"] = ["burnt"]
    with pytest.raises(ValueError, match="penalty_signs"):
        VD.sort_penalty(bad, {"flags": []})


@pytest.mark.parametrize("case, verdict, want", [
    ("rc_flag_on_good_wax", "Good", 1),              # the penalty never changes the verdict
    ("rc_flag_on_borderline_plaster", "Borderline", 1),
    ("rc_flag_on_unlikely_c1", "Unlikely", 1),
    ("rc_lift_blocked_wax_unlikely", "Unlikely", 1),
    ("rc_burnt_only_borderline", "Borderline", 0),
    ("rc_lift_burnt_still_lifts", "Good", 0),
    ("soft_c1_just_over", "Borderline", 0),          # the soft "possible thin coating" note is not a flag
    ("rc_good_positive_all_six", "Good", 0),
])
def test_end_to_end_goldens_follow_the_ruling(case, verdict, want):
    e = GOLD[case]
    for a in ("radiocarbon", "isotopes"):
        assert e[f"{a}.verdict"] == verdict
        assert e[f"{a}.sort_penalty"] == want
    assert "zooms.sort_penalty" not in e
