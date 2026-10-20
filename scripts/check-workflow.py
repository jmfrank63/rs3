#!/usr/bin/env python3
"""Structural check of .github/workflows/ci.yml (D-08; no actionlint on this host).

The workflow path is resolved from this script's location, so a copied tree
(for example the planted-violation self-test under target/tmp) checks its own
ci.yml. Run strings are matched exactly, which rejects an appended `|| true` or
a thread-count flag. Depends only on the standard library and PyYAML.
Prints one `check-workflow: <msg>` line per error to stderr; exits 0 or 1.
"""
import sys
from pathlib import Path

import yaml

PATH = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "ci.yml"
EXPECTED_USES = [
    "actions/checkout@v7",
    "dtolnay/rust-toolchain@1.99.1",
    "Swatinem/rust-cache@v2",
]
REQUIRED_RUNS = [
    "bash scripts/ci-guards.sh",
    "cargo fmt --check",
    "cargo clippy --all-targets --locked -- -D warnings",
    "cargo test --locked",
    "cargo build --release --locked",
    "bash scripts/ci-smoke.sh",
]
# Keys that could weaken or bypass a gate step if set on the job or a step.
FORBIDDEN_KEYS = ("continue-on-error", "env", "if", "shell")


def main() -> int:
    try:
        with open(PATH, encoding="utf-8") as f:
            wf = yaml.safe_load(f)
    except (OSError, yaml.YAMLError) as e:
        print(f"check-workflow: cannot load {PATH}: {e}", file=sys.stderr)
        return 1
    if not isinstance(wf, dict):
        print(f"check-workflow: {PATH} is not a YAML mapping", file=sys.stderr)
        return 1
    errs = []
    # PyYAML implements YAML 1.1: the bare key `on` loads as boolean True.
    triggers = wf.get("on", wf.get(True))
    if not isinstance(triggers, dict) or "push" not in triggers or "pull_request" not in triggers:
        errs.append(f"triggers must include push and pull_request, got {triggers!r}")
    elif triggers.get("push") not in (None, {}):
        errs.append("push trigger must not be filtered (all branches)")
    if wf.get("permissions") != {"contents": "read"}:
        errs.append(f"permissions must be {{contents: read}}, got {wf.get('permissions')!r}")
    conc = wf.get("concurrency") or {}
    if not isinstance(conc, dict) or conc.get("cancel-in-progress") is not True or not conc.get("group"):
        errs.append("concurrency group with cancel-in-progress: true required")
    if (wf.get("env") or {}) != {"CARGO_TERM_COLOR": "always"}:
        errs.append(f"workflow env must be only CARGO_TERM_COLOR, got {wf.get('env')!r}")
    if "defaults" in wf:
        errs.append("workflow: defaults is forbidden")
    jobs = wf.get("jobs") or {}
    if not isinstance(jobs, dict) or list(jobs) != ["ci"]:
        errs.append(f"exactly one job 'ci' expected, got {list(jobs) if isinstance(jobs, dict) else jobs!r}")
        jobs = jobs if isinstance(jobs, dict) else {}
    job = jobs.get("ci") or {}
    if not isinstance(job, dict):
        errs.append(f"job 'ci' must be a mapping, got {job!r}")
        job = {}
    if job.get("runs-on") != "ubuntu-26.04":
        errs.append(f"runs-on must be ubuntu-26.04, got {job.get('runs-on')!r}")
    if job.get("timeout-minutes") != 45:
        errs.append("timeout-minutes: 45 required")
    if "defaults" in job:
        errs.append("job: defaults is forbidden")
    steps = job.get("steps") or []
    if not isinstance(steps, list) or not all(isinstance(s, dict) for s in steps):
        errs.append("job steps must be a list of mappings")
        steps = []
    for scope, obj in [("job", job)] + [(f"step {i}", s) for i, s in enumerate(steps)]:
        for key in FORBIDDEN_KEYS:
            if key in obj:
                errs.append(f"{scope}: {key} is forbidden")
    uses = [s["uses"] for s in steps if "uses" in s]
    if uses != EXPECTED_USES:
        errs.append(f"uses must be exactly {EXPECTED_USES} in order, got {uses}")
    for s in steps:
        if str(s.get("uses", "")).startswith("dtolnay/rust-toolchain"):
            comps = {c.strip() for c in str((s.get("with") or {}).get("components", "")).split(",")}
            if not {"rustfmt", "clippy"} <= comps:
                errs.append("dtolnay/rust-toolchain needs components: rustfmt, clippy")
    runs = [str(s["run"]).strip() for s in steps if "run" in s]
    pos = [runs.index(r) if r in runs else -1 for r in REQUIRED_RUNS]
    for r, p in zip(REQUIRED_RUNS, pos):
        if p < 0:
            errs.append(f"missing run step: {r!r}")
    if all(p >= 0 for p in pos) and pos != sorted(pos):
        errs.append(f"run steps out of order: {runs}")
    elif all(p >= 0 for p in pos) and runs != REQUIRED_RUNS:
        errs.append(f"run steps must be exactly {REQUIRED_RUNS}, got {runs}")
    if any("actions-rs/" in str(u) for u in uses):
        errs.append("actions-rs/* is forbidden")
    for e in errs:
        print(f"check-workflow: {e}", file=sys.stderr)
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main())
