"""Write crates/spyder-core/tests/goldens/oracle_csv_v1.json: the frozen oracle's CSV rows (oracle.export.csv_row,
cells as oracle.export._cell writes them) for every PUBLIC end-to-end golden case (reference/oracle/goldens) and
all three analysis profiles, plus the oracle's CSV file text for the radiocarbon rows (UTF-8 with BOM, CRLF).
The `file` and `dependencies` columns are blanked (they depend on the path and the session). Public data only.

    python crates/spyder-core/tests/gen/gen_oracle_csv.py
"""
import base64
import json
import sys
import tempfile
from pathlib import Path

import numpy as np

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).parent))
from parity_common import REPO  # noqa: E402

sys.path.insert(0, str(REPO / "reference"))
from oracle import Oracle  # noqa: E402
from oracle import export as EX  # noqa: E402

GOLD = REPO / "reference" / "oracle" / "goldens"
OUT = REPO / "crates" / "spyder-core" / "tests" / "goldens" / "oracle_csv_v1.json"
WL = np.arange(350, 2501).astype(float)


def main():
    o = Oracle([str(REPO / "plugins")])
    E = json.loads((GOLD / "oracle_public_v1.json").read_text(encoding="utf-8"))
    sp = {s["sha256"]: np.asarray(s["values"], float)
          for s in json.loads((GOLD / E["spectra_file"]).read_text(encoding="utf-8"))["spectra"]}
    cases, rc_rows = [], []
    for c in E["cases"]:
        if "asd_b64" in c:
            rec = o.analyse_file(base64.b64decode(c["asd_b64"]), c["context"]["instrument_class"],
                                 c["context"].get("class_source", "user"), name=c["name"] + ".asd")
        else:
            rec = o.analyse_spectrum(WL, sp[c["spectrum_sha256"]], c["context"])
        rows = {}
        for a in ("radiocarbon", "isotopes", "zooms"):
            r = EX.csv_row(rec, a)
            r["file"] = c["name"]
            r["dependencies"] = ""
            rows[a] = [[k, EX._cell(v)] for k, v in r.items()]
            if a == "radiocarbon":
                rc_rows.append(r)
        cases.append({"name": c["name"], "rows": rows})
    with tempfile.TemporaryDirectory() as td:
        p = Path(td) / "rc.csv"
        EX.write_csv(rc_rows, p)
        text = p.read_bytes().decode("utf-8")
    OUT.write_text(json.dumps({"format": "spyder-bone/oracle-csv", "version": 1,
                               "generated_by": "crates/spyder-core/tests/gen/gen_oracle_csv.py (reference/oracle)",
                               "cases": cases, "radiocarbon_csv": text}, allow_nan=False) + "\n", encoding="utf-8")
    print(f"wrote {OUT.relative_to(REPO)}: {len(cases)} cases x 3 profiles")


if __name__ == "__main__":
    main()
