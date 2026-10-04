"""Golden split at export (PLAN v3.2 Phase 0b item 14).

* Shipped files carry PUBLIC goldens only (cases whose spectra are public: JAS mmc2, Archaeometry SI, synthetic,
  numbered standard/high-res pairs).
* Private cases live in a separate set keyed by the file's `id@version` AND its BODY hash, so a private set can never
  be run against a different body by accident.
* BODY hash = sha256 of the canonical JSON of the file with the `golden` and `version` members removed (canonical =
  sorted keys, separators (',', ':'), UTF-8, no ASCII escaping). Golden-only edits leave it unchanged.
* Versions: a golden-only change bumps PATCH; a parameter (body) change bumps MAJOR (predictions may change).
* Public provenance text is anonymised (no site, project or specimen names); `public_text_problems` flags names.

CLI:  python -m oracle.split body <file.json>        print the body hash
      python -m oracle.split split <file.json> <public_hashes.json> <out_public.json> <out_private_set.json>
"""
from __future__ import annotations

import copy
import hashlib
import json
import re
import sys

def _load_private_names() -> list[str]:
    """Names that must not appear in public text. The list itself is private: it is read from the file named by
    SPYDER_PRIVATE_NAMES (one name per line, lower case), or from the private planning folder when it exists. Without
    it the name check is skipped (and says so)."""
    import os
    from pathlib import Path
    cands = [os.environ.get("SPYDER_PRIVATE_NAMES", ""),
             str(Path(__file__).resolve().parents[2] / "planning" / "work" / "phase0b" / "oracle" / "private_names.txt")]
    for c in cands:
        if c and Path(c).is_file():
            return [ln.strip().lower() for ln in Path(c).read_text(encoding="utf-8").splitlines() if ln.strip()]
    print("oracle.split: no private-names list found; public-text name check skipped", file=sys.stderr)
    return []


PRIVATE_NAMES = _load_private_names()
SKIP_TEXT_KEYS = {"values", "asd_b64", "sha256", "spectrum_sha256", "input_sha256"}


def canonical(doc) -> bytes:
    return json.dumps(doc, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def body_sha256(doc) -> str:
    d = {k: v for k, v in doc.items() if k not in ("golden", "version")}
    return hashlib.sha256(canonical(d)).hexdigest()


def golden_sha256(doc) -> str:
    return hashlib.sha256(canonical(doc.get("golden", {}))).hexdigest()


def bump(version, part):
    core = str(version).split("-")[0].split("+")[0]
    a, b, c = (int(x) for x in (core.split(".") + ["0", "0"])[:3])
    if part == "major":
        return f"{a + 1}.0.0"
    if part == "minor":
        return f"{a}.{b + 1}.0"
    return f"{a}.{b}.{c + 1}"


def _strings(o, key=None):
    if isinstance(o, dict):
        for k, v in o.items():
            if k in SKIP_TEXT_KEYS:
                continue
            yield str(k)
            yield from _strings(v, k)
    elif isinstance(o, list):
        for v in o:
            yield from _strings(v, key)
    elif isinstance(o, str):
        yield o


def public_text_problems(doc, allow=()):
    """Private site / project names found in the file's TEXT (keys and string values; numbers, spectra, file
    bytes and hashes are not text). Word-boundary match, case-insensitive."""
    txt = "\n".join(_strings(doc)).lower()
    return [n for n in PRIVATE_NAMES if n not in allow and re.search(r"(?<![a-z0-9])" + re.escape(n) + r"(?![a-z0-9])", txt)]


def split(doc, public_spectrum_hashes):
    """Return (public_doc, private_set) from a document whose golden cases may reference private spectra."""
    pub = copy.deepcopy(doc)
    cases = doc.get("golden", {}).get("cases", [])
    keep = [c for c in cases if c.get("spectrum_sha256") in public_spectrum_hashes or c.get("spectrum_sha256") is None]
    priv = [c for c in cases if c not in keep]
    pub.setdefault("golden", {})["cases"] = keep
    private_set = {"format": "spyder-bone/private_goldens", "format_version": 1,
                   "for": f"{doc.get('id')}@{doc.get('version')}", "body_sha256": body_sha256(doc), "cases": priv}
    return pub, private_set


def private_set_applies(private_set, doc):
    fid = private_set["for"].split("@")[0]
    return fid == doc.get("id") and private_set["body_sha256"] == body_sha256(doc)


if __name__ == "__main__":
    a = sys.argv[1:]
    if a and a[0] == "body":
        print(body_sha256(json.load(open(a[1], encoding="utf-8"))))
    elif a and a[0] == "split":
        d = json.load(open(a[1], encoding="utf-8")); hs = set(json.load(open(a[2], encoding="utf-8")))
        p, s = split(d, hs)
        json.dump(p, open(a[3], "w", encoding="utf-8"), indent=1, ensure_ascii=False)
        json.dump(s, open(a[4], "w", encoding="utf-8"), indent=1, ensure_ascii=False)
    else:
        print(__doc__)
