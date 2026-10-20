#!/usr/bin/env python3
"""Checks of .github/workflows/ci.yml (D-08; no actionlint on this host).

Usage: check-workflow.py [--lines] [--structure]   (no argument: both checks)

--lines      Standard library only. ci.yml must be printable ASCII, and its non-blank,
             non-comment lines must equal CANONICAL below, in order: an allow-list of the
             whole text, not a block-list. Any structural edit fails it: a new key (`if:`,
             `shell:`, `env:`, `with: ref:`), a changed or suffixed run string (`|| true`,
             a dropped `-D warnings`, a thread-count flag), a deleted step or permissions
             block, explicit-key (`? ref`) or flow-style YAML, a second `${{` expression.
             It also bans `continue-on-error` and `SERVER.` (any case) on those lines
             (D-05 (g)). This is the only workflow check in CI when PyYAML is missing.
--structure  Needs PyYAML. Parses the workflow and checks the policy: allow-listed keys,
             exact action refs and run strings, triggers, permissions, concurrency. It also
             guards CANONICAL: a gate-weakening edit made to ci.yml and CANONICAL together
             passes --lines but still fails here (scripts/ci-guards.sh makes it mandatory
             outside CI).

EDITING ci.yml MEANS UPDATING CANONICAL IN THE SAME CHANGE (comments and blank lines are
free; every other line is pinned). scripts/test-ci-guards.sh proves the current ci.yml
passes both checks.

The workflow path is resolved from this script's location, so a copied tree (for example
the planted-violation self-test under target/tmp) checks its own ci.yml.
Prints `check-workflow: <msg>` per error to stderr; exits 0 if every selected check passes.
"""
import difflib
import re
import sys
from pathlib import Path

PATH = Path(__file__).resolve().parent.parent / ".github" / "workflows" / "ci.yml"
# Every non-blank, non-comment line of ci.yml, exactly (trailing spaces aside), in order.
# None opens a block scalar (`|`, `>`) or a quoted scalar, so every line that starts with
# `#` really is a YAML comment and can be skipped safely.
CANONICAL = [
    "name: CI",
    "on:",
    "  push:",
    "  pull_request:",
    "  workflow_dispatch:",
    "permissions:",
    "  contents: read",
    "concurrency:",
    "  group: ci-${{ github.ref }}",
    "  cancel-in-progress: true",
    "env:",
    "  CARGO_TERM_COLOR: always",
    "jobs:",
    "  ci:",
    "    name: fmt, clippy, test, release smoke (x86_64)",
    "    runs-on: ubuntu-26.04",
    "    timeout-minutes: 45",
    "    steps:",
    "      - name: Checkout",
    "        uses: actions/checkout@v7",
    "      - name: Install Rust 1.99.1",
    "        uses: dtolnay/rust-toolchain@1.99.1",
    "        with:",
    "          components: rustfmt, clippy",
    "      - name: Cache cargo and target",
    "        uses: Swatinem/rust-cache@v2",
    "      - name: Repo guards",
    "        run: bash scripts/ci-guards.sh",
    "      - name: cargo fmt --check",
    "        run: cargo fmt --check",
    "      - name: cargo clippy",
    "        run: cargo clippy --all-targets --locked -- -D warnings",
    "      - name: cargo test",
    "        run: cargo test --locked",
    "      - name: cargo build --release",
    "        run: cargo build --release --locked",
    "      - name: Release smoke",
    "        run: bash scripts/ci-smoke.sh",
]
# D-05 (g), on the functional lines (comments may say "server." in plain English).
D05_BANNED = re.compile(r"continue-on-error|server\.", re.IGNORECASE)
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
# Allow-lists: anything else could redirect or neuter the gates (checkout `ref:`/`repository:`,
# `working-directory`, job-level `permissions`, `container`, `services`, a `toolchain:` override,
# trigger filters). PyYAML loads the bare key `on` as True, so both spellings are allowed.
WORKFLOW_KEYS = {"name", "on", True, "permissions", "concurrency", "env", "jobs"}
TRIGGERS = {"push", "pull_request", "workflow_dispatch"}
CONCURRENCY_KEYS = {"group", "cancel-in-progress"}
CONCURRENCY_GROUP = "ci-${{ github.ref }}"
JOB_KEYS = {"name", "runs-on", "timeout-minutes", "steps"}
STEP_KEYS = {"name", "uses", "with", "run"}
ALLOWED_WITH = {
    "actions/checkout@v7": set(),
    "dtolnay/rust-toolchain@1.99.1": {"components"},
    "Swatinem/rust-cache@v2": set(),
}


def unexpected(obj, allowed):
    """Keys of obj outside allowed; FORBIDDEN_KEYS and defaults are reported separately."""
    return sorted(str(k) for k in set(obj) - allowed - set(FORBIDDEN_KEYS) - {"defaults"})


def lines_errors(text):
    """--lines: printable ASCII only, and the functional lines equal CANONICAL exactly."""
    errs = []
    lines = text.split("\n")
    # Only "\n" separates lines here, and nothing else may hide in a line: YAML and Python
    # disagree on other line breaks (\x85,  , \x0c, ...), and a tab or CR is never needed.
    bad = [n for n, line in enumerate(lines, 1) if any(not " " <= ch <= "~" for ch in line)]
    if bad:
        errs.append(f"non-printable or non-ASCII characters (tab, CR, ...) on lines {bad}")
    got = [line.rstrip() for line in lines if line.strip() and not line.lstrip().startswith("#")]
    for line in got:
        if D05_BANNED.search(line):
            errs.append(f"continue-on-error / SERVER. is forbidden in the workflow: {line.strip()!r}")
    if got != CANONICAL:
        diff = "\n".join(difflib.unified_diff(
            CANONICAL, got, "CANONICAL (scripts/check-workflow.py)", str(PATH), lineterm="", n=1))
        errs.append("workflow lines differ from the canonical ci.yml (non-blank, non-comment lines;"
                    " editing ci.yml means updating CANONICAL in scripts/check-workflow.py):\n" + diff)
    return errs


def structure_errors(text):
    """--structure: parse with PyYAML and check the policy."""
    try:
        import yaml
    except ImportError as e:
        return [f"PyYAML is required for --structure ({e}); --lines needs only the standard library"]
    try:
        wf = yaml.safe_load(text)
    except yaml.YAMLError as e:
        return [f"cannot parse {PATH}: {e}"]
    if not isinstance(wf, dict):
        return [f"{PATH} is not a YAML mapping"]
    errs = []
    if extra := unexpected(wf, WORKFLOW_KEYS):
        errs.append(f"workflow: unexpected keys {extra}")
    # PyYAML implements YAML 1.1: the bare key `on` loads as boolean True.
    triggers = wf.get("on", wf.get(True))
    if not isinstance(triggers, dict) or "push" not in triggers or "pull_request" not in triggers:
        errs.append(f"triggers must include push and pull_request, got {triggers!r}")
    else:
        if extra := sorted(str(t) for t in set(triggers) - TRIGGERS):
            errs.append(f"unexpected triggers {extra} (allowed: {sorted(TRIGGERS)})")
        for t in sorted(TRIGGERS):
            if triggers.get(t) not in (None, {}):
                errs.append(f"{t} trigger must not be filtered, got {triggers.get(t)!r}")
    if wf.get("permissions") != {"contents": "read"}:
        errs.append(f"permissions must be {{contents: read}}, got {wf.get('permissions')!r}")
    conc = wf.get("concurrency") or {}
    if not isinstance(conc, dict) or conc.get("cancel-in-progress") is not True or not conc.get("group"):
        errs.append("concurrency group with cancel-in-progress: true required")
    else:
        if extra := unexpected(conc, CONCURRENCY_KEYS):
            errs.append(f"concurrency: unexpected keys {extra}")
        # Per ref (D-03): a constant group would let any push cancel runs on every other ref.
        if conc.get("group") != CONCURRENCY_GROUP:
            errs.append(f"concurrency group must be {CONCURRENCY_GROUP!r}, got {conc.get('group')!r}")
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
    if extra := unexpected(job, JOB_KEYS):
        errs.append(f"job: unexpected keys {extra}")
    for i, s in enumerate(steps):
        if extra := unexpected(s, STEP_KEYS):
            errs.append(f"step {i}: unexpected keys {extra}")
        if ("uses" in s) == ("run" in s):
            errs.append(f"step {i}: needs exactly one of uses or run")
        with_ = s.get("with") or {}
        if not isinstance(with_, dict):
            errs.append(f"step {i}: with must be a mapping, got {with_!r}")
        elif "run" in s and with_:
            errs.append(f"step {i}: with is only allowed on uses steps")
        elif "uses" in s:
            if extra := sorted(set(map(str, with_)) - ALLOWED_WITH.get(str(s["uses"]), set())):
                errs.append(f"step {i}: unexpected with: inputs {extra} for {s['uses']}")
    uses = [s["uses"] for s in steps if "uses" in s]
    if uses != EXPECTED_USES:
        errs.append(f"uses must be exactly {EXPECTED_USES} in order, got {uses}")
    for s in steps:
        if str(s.get("uses", "")).startswith("dtolnay/rust-toolchain"):
            with_ = s.get("with") if isinstance(s.get("with"), dict) else {}
            comps = {c.strip() for c in str(with_.get("components", "")).split(",")}
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
    return errs


def main(argv) -> int:
    modes = set(argv) or {"--lines", "--structure"}
    if not modes <= {"--lines", "--structure"}:
        print(f"check-workflow: usage: {Path(__file__).name} [--lines] [--structure]", file=sys.stderr)
        return 2
    try:
        text = PATH.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as e:
        print(f"check-workflow: cannot read {PATH}: {e}", file=sys.stderr)
        return 1
    errs = []
    if "--lines" in modes:
        errs += lines_errors(text)
    if "--structure" in modes:
        errs += structure_errors(text)
    for e in errs:
        print(f"check-workflow: {e}", file=sys.stderr)
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
