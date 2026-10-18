"""B6 is cut-aware (DECISIONS 109): expected values written from the rule, not from running the code (LESSONS 44)."""
import sys
from pathlib import Path

REF = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REF))
from oracle.pipeline import b6_noise  # noqa: E402

P = {"cuts": [0.5, 3.0], "check_if_cut_within_sd": 2.6}


def test_near_a_cut_is_a_check():
    # |3.4 - 3| = 0.4 <= 2.6 x 0.2 = 0.52
    assert b6_noise(3.4, 0.2, P) == {"status": "assessed", "outcome": "Check"}
    # |0.1 - 0.5| = 0.4 <= 2.6 x 0.16 = 0.416
    assert b6_noise(0.1, 0.16, P)["outcome"] == "Check"


def test_far_from_every_cut_is_ok_even_when_noisy():
    # the old rule (SD > 0.5) raised a Check here; |8 - 3| = 5 > 2.6 x 1.5 = 3.9
    assert b6_noise(8.0, 1.5, P)["outcome"] == "ok"
    # |1.7 - 0.5| = 1.2 and |1.7 - 3| = 1.3, both > 2.6 x 0.4 = 1.04
    assert b6_noise(1.7, 0.4, P)["outcome"] == "ok"


def test_very_noisy_reading_reaches_a_cut_from_far_away():
    # |12 - 3| = 9 <= 2.6 x 4 = 10.4
    assert b6_noise(12.0, 4.0, P)["outcome"] == "Check"


def test_no_sd_or_no_reading():
    assert b6_noise(2.0, None, P) == {"status": "not assessed: no noise gain for this model and transfer", "outcome": None}
    assert b6_noise(None, 0.3, P) == {"status": "assessed", "outcome": None}
    assert b6_noise(float("nan"), 0.3, P)["outcome"] is None
