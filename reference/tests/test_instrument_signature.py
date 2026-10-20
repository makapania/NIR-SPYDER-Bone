"""An instrument is known by its file format AND its detector joins, never by the format alone (DECISIONS 137,
instruments.json 2.1.0). as7 is ASD's file version 7: the Trek-type handheld writes it (blank header joins, the
SWIR1/SWIR2 join found in the white-reference DN at 1785 nm), and a LabSpec 4 on older ASD software may write it (joins
1000/1800, as on every LabSpec 4 as8 file). An as7 file with the LabSpec joins is scored exactly like the same scan in
as8: the switch decides its class, no uncalibrated note. Any other as7 file stays the Trek-type class, as does one
whose joins could not be found (a fallback says nothing about the instrument). The same cases run against the Rust
pipeline in crates/spyder-core/tests/instrument_signature.rs. Synthetic files on a public spectrum."""
from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

REF = Path(__file__).resolve().parents[1]
REPO = REF.parent
PLUG = REPO / "plugins"
sys.path.insert(0, str(REF))
from oracle import Oracle                    # noqa: E402
from oracle import asd as ASD                # noqa: E402
from oracle.params import PluginSet          # noqa: E402

WL = np.arange(350, 2501).astype(float)
STD, HIRES, TREK, FS3 = "asd.labspec4.std", "asd.labspec4.hires", "asd.trek", "asd.fieldspec3"


def bone():
    g = json.loads((PLUG / "golden_spectra_public_v2.json").read_text(encoding="utf-8"))
    return np.asarray(next(s for s in g["spectra"] if s["name"] == "mmc2_Reference_Sample_44")["values"], float)


def reference(j1=1000.0, j2=1800.0):
    """A white-reference DN curve with its detector steps after j1 and j2 nm."""
    b = 20000.0 + 8000.0 * np.exp(-(((WL - 1300.0) / 600.0) ** 2))
    return np.where(WL <= j1, b * 0.8, np.where(WL <= j2, b, b * 0.6))


def files(joins, ref):
    """The same scan as as7 (the Trek-type layout, dark flag 0) and as8, with these header joins."""
    x = bone()
    a7 = ASD.write_as7(x, ref, 12345, 46273.5707, 46273.5786, 1788889308, joins=joins)
    a8 = ASD.write_as8(x, ref, 12345, 46273.5707, 46273.5786, 1788889308, joins=joins)
    return a7, a8


def keys(rec):
    return [n["key"] for n in rec["instrument"]["notes"]]


def test_the_registry_names_as7_by_its_detector_joins():
    P = PluginSet([str(PLUG)])
    assert P.find("spyder-bone/instrument").version == "2.1.0"
    assert P.class_of_file("as7", [1000.0, 1800.0], "header") is None
    assert P.class_of_file("as7", [1000.0, 1800.0], "reference_dn") is None
    assert P.class_of_file("as8", [1000.0, 1800.0], "header") is None
    assert P.class_of_file("as7", [1000.0, 1785.0], "reference_dn") == TREK
    assert P.class_of_file("as7", [1000.0, 1830.0], "header") == TREK
    assert P.class_of_file("as7", [1001.0, 1800.0], "header") == TREK
    # no joins found: the reader's 1000/1800 fallback is not evidence of a LabSpec 4
    assert P.class_of_file("as7", [1000.0, 1800.0], "default") == TREK
    assert P.class_of_file("as8", [1000.0, 1785.0], "reference_dn") is None
    assert P.class_of_file("as8", [1000.0, 1800.0], "default") is None
    assert P.class_of_file("legacy", [1000.0, 1800.0], "header") == FS3
    assert P.class_of_file("legacy", [1000.0, 1800.0], "default") == FS3


def test_an_as7_file_with_the_labspec_joins_is_scored_exactly_like_as8():
    o = Oracle([str(PLUG)])
    a7, a8 = files((1000.0, 1800.0), reference())
    for cls in (STD, HIRES):
        r7 = o.analyse_file(a7, cls, "user", name="a.asd")
        r8 = o.analyse_file(a8, cls, "user", name="a.asd")
        assert r7["input"]["format"] == "as7" and "joins_source" not in r7["input"]
        assert r7["instrument"]["class"] == cls and r7["instrument"]["class_source"] == "user"
        assert "instrument_uncalibrated" not in keys(r7)
        assert list(r7) == list(r8)
        for k in r8:
            if k != "input":
                assert json.dumps(r7[k], sort_keys=True, default=str) == json.dumps(r8[k], sort_keys=True, default=str), (cls, k)
    assert o.analyse_file(a7, HIRES, "user", name="a.asd")["stream"]["transfer"] != "none"
    # a blank header whose white reference shows the LabSpec joins: found joins 1000/1800, a LabSpec file too
    blank, _ = files((0.0, 0.0), reference())
    r = o.analyse_file(blank, STD, "user", name="b.asd")
    assert r["input"]["joins_source"] == "reference_dn"
    assert (r["instrument"]["class"], r["instrument"]["class_source"]) == (STD, "user")


def test_the_trek_signature_and_other_as7_joins_keep_the_trek_class():
    o = Oracle([str(PLUG)])
    cases = [("trek", (0.0, 0.0), reference(1000.0, 1785.0), "reference_dn"),
             ("other", (1000.0, 1830.0), reference(), "header"),
             ("none", (0.0, 0.0), np.full(len(WL), 20000.0), "default")]
    for tag, joins, ref, source in cases:
        a7, a8 = files(joins, ref)
        for cls in (STD, HIRES):
            r = o.analyse_file(a7, cls, "user", name="t.asd")
            assert (r["instrument"]["class"], r["instrument"]["class_source"]) == (TREK, "file_format"), tag
            assert "instrument_uncalibrated" in keys(r), tag
            assert r["stream"]["transfer"] == "none", tag
            assert r["input"].get("joins_source", "header") == source, tag
        if tag == "trek":
            # the same Trek-type scan in as8 is a LabSpec file (as8 is never another instrument's)
            assert o.analyse_file(a8, STD, "user", name="t8.asd")["instrument"]["class"] == STD


def test_the_schema_takes_unless_joins_only_with_a_file_format():
    import copy
    import pytest
    from spyder_ref.validate_plugins import _schema_validator
    v = _schema_validator()
    if v is None:
        pytest.skip("jsonschema not installed")
    doc = json.loads((PLUG / "instruments.json").read_text(encoding="utf-8"))
    assert v(doc) == []
    for hint, ok in (({"file_format": ["as7"], "unless_joins_nm": [1000, 1800]}, True),
                     ({"unless_joins_nm": [1000, 1800]}, False),
                     ({"file_format": ["as7"], "unless_joins_nm": []}, False),
                     ({"file_format": ["as7"], "unless_joins_nm": ["1000"]}, False)):
        d = copy.deepcopy(doc)
        d["classes"][TREK]["header_hint"] = hint
        assert (v(d) == []) == ok, hint
