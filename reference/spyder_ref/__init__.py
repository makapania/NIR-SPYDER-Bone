"""spyder_ref: the frozen Python reference engine for SPYDER Bone plug-in files (numpy only at runtime).

    import spyder_ref as SR
    SR.validate_file("plugins/collagen_consensus3_median.spyder-model.json")
    SR.predict(SR.load_json(path), wl, R, SR.ScanContext((1000.0, 1800.0), "asd.labspec4.std"))

Modules: core (operators, model/transfer/consensus files, golden runner), n2 (the canonical noise measure N2),
plugins (validation of a whole plug-in folder: schemas, goldens, cross-file hashes). export and golden_gen are
developer tools (they need scipy; never imported by the engine).
"""
from .core import *          # noqa: F401,F403
from .core import (_rule_feature, _segments, _check_snv_blocks, _sg_matrix)   # noqa: F401  (used by tests and tools)
from .n2 import n2, n_centres  # noqa: F401

__version__ = "1.1.0"        # engine / op-set 1.1 (block SNV, consensus kind)
