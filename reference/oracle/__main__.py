"""CLI:  python -m oracle analyse <file.asd ...> --class std|hires [--plugins plugins] [--analysis radiocarbon]
                                 [--csv out.csv] [--manifest out.jsonl]
       python -m oracle goldens [--plugins plugins]          re-run every stored public golden (exit 1 on failure)
(run with reference/ on PYTHONPATH)"""
import argparse
import json
import sys

from . import export
from .make_goldens import run_stored
from .pipeline import Oracle

CLS = {"std": "asd.labspec4.std", "hires": "asd.labspec4.hires"}


def main(argv=None):
    ap = argparse.ArgumentParser(prog="oracle")
    sub = ap.add_subparsers(dest="cmd", required=True)
    a = sub.add_parser("analyse")
    a.add_argument("files", nargs="+")
    a.add_argument("--class", dest="cls", choices=list(CLS), required=True)
    a.add_argument("--plugins", nargs="+", default=["plugins"])
    a.add_argument("--analysis", default="radiocarbon", choices=["radiocarbon", "isotopes", "zooms"])
    a.add_argument("--csv")
    a.add_argument("--manifest")
    g = sub.add_parser("goldens")
    g.add_argument("--plugins", nargs="+", default=["plugins"])
    x = ap.parse_args(argv)
    o = Oracle(x.plugins)
    if x.cmd == "goldens":
        return 1 if run_stored(o, x.plugins[0]) else 0
    rows, man = [], []
    for f in x.files:
        rec = o.analyse_file(f, CLS[x.cls], "user")
        rows.append(export.csv_row(rec, x.analysis))
        man.append(export.manifest(rec, x.analysis))
        r = rec[x.analysis]
        print(f"{f}: {r['verdict']} ({r['rule_step']}) notes {r.get('notes_shown')} flags {[q['key'] for q in r.get('flags', [])]}")
    if x.csv:
        export.write_csv(rows, x.csv)
    if x.manifest:
        with open(x.manifest, "w", encoding="utf-8") as fh:
            for m in man:
                fh.write(json.dumps(m) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
