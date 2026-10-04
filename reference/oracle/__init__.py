"""SPYDER Bone end-to-end oracle (PLAN v3.2 Phase 0b item 7): the frozen Python implementation of Steps 1-10.

Pure numpy; every parameter is loaded from plug-in files (params.PluginSet); model and transfer chains run through
the reference engine spyder_ref. The Rust CLI (Phase 3) must reproduce its CSV rows: numbers to 1e-9, labels exactly.

    from oracle import Oracle
    o = Oracle(["plugins"])
    rec = o.analyse_file("scan.asd", instrument_class="asd.labspec4.hires")
    row = oracle.export.csv_row(rec, "radiocarbon")
"""
from .pipeline import ORACLE_VERSION, Oracle  # noqa: F401
