"""Re-run every Phase 2 and Phase 3 parity check (PLAN section 8 gates; release gate 1) in one command:

  1. Python side: reference/spyder_ref validates the plug-in folder as packaged, and the frozen oracle re-runs
     every stored public golden (end-to-end, engine checks, profiles: `oracle.make_goldens --check`).
  2. Rust side: `spyder validate plugins --json`; every file must load, and every file with goldens must run
     exactly as many golden comparisons in Rust as in Python (same goldens, same counting).
  3. Public fixtures regenerated from Python (deterministic; reported if they changed): the fuzz predictions
     (200 perturbed public spectra) and the oracle's CSV rows for the 54 end-to-end golden cases.
  4. Private (only when SPYDER_PRIVATE_DATA is set): regenerate planning/work/phase2/private_predictions.json and
     planning/work/phase3/private_oracle.json (the real .asd files; stay in planning/).
  5. cargo fmt --check, cargo clippy --all-targets -D warnings, cargo test (spyder-core, spyder-cli), with
     SPYDER_PRIVATE_DATA passed through (the private tests run only when it is set).

Run from anywhere (Windows paths shown; any OS works):
    python crates/spyder-core/tests/gen/run_parity.py
    set SPYDER_PRIVATE_DATA=<private data folder>   (to include the private parity)
Exit code 0 = every check passed.
"""
import json
import os
import re
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from parity_common import REPO, SR  # noqa: E402,F401

sys.path.insert(0, str(REPO / "reference"))
from spyder_ref import validate_plugins as VP  # noqa: E402

PLUGINS = REPO / "plugins"
PY = sys.executable
CRATES = ["-p", "spyder-core", "-p", "spyder-cli"]
problems = []


def step(title):
    print(f"\n=== {title}", flush=True)


def run(cmd, env=None, **kw):
    print("$ " + " ".join(str(c) for c in cmd), flush=True)
    return subprocess.run(cmd, cwd=REPO, env=env, **kw)


def main():
    env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1",)

    step("1. Python reference (spyder_ref) validates the plug-in folder")
    bad, report = VP.validate_folder(PLUGINS, verbose=True)
    if bad:
        problems.append(f"spyder_ref: {bad} problem(s) in plugins/")
    r = run([PY, "-m", "oracle.make_goldens", "--roots", "plugins", "--check"],
            env=dict(env, PYTHONPATH=str(REPO / "reference")), capture_output=True, text=True)
    print(r.stdout[-2500:])
    if r.returncode:
        problems.append("oracle: stored public goldens fail in Python")
    oracle_counts = {}
    for line in r.stdout.splitlines():
        m = re.match(r"(check|profile)\.(\w+)@[\d.]+: (\d+) comparisons, (\d+) failed", line)
        if m:
            oracle_counts[f"{m.group(1)}.{m.group(2)}"] = (int(m.group(3)), int(m.group(4)))
    py_counts = {}
    for p, status, msg in report:
        m = re.search(r"goldens (\d+) checks, (\d+) failed", msg)
        if m and status == "checked":
            py_counts[Path(p).resolve()] = (int(m.group(1)), int(m.group(2)))

    step("2. Rust engine (spyder validate) on the same folder; golden check counts compared")
    r = run(["cargo", "build", "-q", "-p", "spyder-cli"], env=env)
    if r.returncode:
        problems.append("cargo build failed")
        return
    tdir = Path(os.environ.get("CARGO_TARGET_DIR") or (REPO / "target"))      # honour CARGO_TARGET_DIR (LESSONS 42)
    exe = (tdir if tdir.is_absolute() else REPO / tdir) / "debug" / ("spyder.exe" if os.name == "nt" else "spyder")
    r = run([str(exe), "validate", str(PLUGINS), "--json"], env=env, capture_output=True, text=True)
    doc = json.loads(r.stdout)
    if r.returncode or not doc["ok"]:
        problems.append("spyder validate: not every bundled file loads (or startup-error state)")
    n_cmp = 0
    for f in doc["files"]:
        name = Path(f["path"]).name
        rust = f["golden_checks"]
        py = py_counts.get(Path(f["path"]).resolve())
        if py is None and f.get("id") in oracle_counts:
            py = oracle_counts[f["id"]]
        mark = ""
        if rust is not None and py is not None:
            n_cmp += 1
            if rust != py[0] or py[1] != 0 or f["golden_failed"]:
                mark = "  <-- MISMATCH"
                problems.append(f"{name}: Rust {rust} checks vs spyder_ref {py[0]} ({py[1]} failed)")
        print(f"  {f['state']:<8} {name:<52} rust {rust!s:>6}  python {py[0] if py else '-'!s:>6}{mark}")
        if f["state"] != "loaded":
            problems.append(f"{name}: {f['error']}")
    print(f"  {n_cmp} files with goldens compared")

    step("3. Fuzz fixture (public): regenerate from spyder_ref")
    fx = REPO / "crates" / "spyder-core" / "tests" / "goldens" / "fuzz_predictions_v1.json"
    before = fx.read_bytes() if fx.exists() else b""
    r = run([PY, str(HERE / "gen_fuzz_predictions.py")], env=env)
    if r.returncode:
        problems.append("gen_fuzz_predictions.py failed")
    print("  fixture " + ("unchanged" if fx.read_bytes() == before else "REGENERATED (plug-ins or reference changed)"))

    fx = REPO / "crates" / "spyder-core" / "tests" / "goldens" / "oracle_csv_v1.json"
    before = fx.read_bytes() if fx.exists() else b""
    r = run([PY, str(HERE / "gen_oracle_csv.py")], env=env)
    if r.returncode:
        problems.append("gen_oracle_csv.py failed")
    print("  oracle CSV fixture " + ("unchanged" if fx.read_bytes() == before else "REGENERATED"))

    step("4. Private real-spectra values")
    if os.environ.get("SPYDER_PRIVATE_DATA"):
        for g in ("phase2/gen_private_predictions.py", "phase3/gen_private_oracle.py"):
            r = run([PY, str(REPO / "planning" / "work" / g)], env=env)
            if r.returncode:
                problems.append(f"{g} failed")
    else:
        print("  SKIPPED: SPYDER_PRIVATE_DATA is not set")

    step("5. cargo fmt / clippy / test")
    for cmd in (["cargo", "fmt", *CRATES, "--", "--check"],
                ["cargo", "clippy", *CRATES, "--all-targets", "--", "-D", "warnings"],
                ["cargo", "test", *CRATES]):
        r = run(cmd, env=env)
        if r.returncode:
            problems.append(" ".join(cmd[:2]) + " failed")


if __name__ == "__main__":
    main()
    print("\n=== " + ("PARITY OK" if not problems else f"PARITY FAILED: {len(problems)} problem(s)"))
    for p in problems:
        print("  - " + p)
    sys.exit(1 if problems else 0)
