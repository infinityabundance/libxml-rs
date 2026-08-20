#!/usr/bin/env python3
"""
Court Runner — Differential testing framework (§40, §44).

Executes a case against both the oracle (upstream Docker image) and the
candidate (libxml-rs artifacts), captures observations, computes residuals,
and emits receipts.

Workflow:
  1. court build-oracle <version>    — build Docker oracle image
  2. court build-candidate           — build libxml-rs artifacts
  3. court run <casefile>            — run a single case
  4. court compare <casefile>        — compare oracle vs candidate output
  5. court sweep <suite>             — run all cases in a suite directory

Casefiles are JSON files conforming to courts/schema.json.
Receipts are written to courts/receipts/<case_id>/<timestamp>.json.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
COURTS_DIR = ROOT / "courts"
RECEIPTS_DIR = COURTS_DIR / "receipts"
ORACLE_DIR = ROOT / "oracle"

# ── Utilities ────────────────────────────────────────────────────────────


def hash_bytes(data):
    return hashlib.sha256(data).hexdigest()


def read_file(path):
    with open(path, "rb") as f:
        return f.read()


def write_json(path, obj):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w") as f:
        json.dump(obj, f, indent=2, sort_keys=True)


def load_json(path):
    with open(path) as f:
        return json.load(f)


def git_commit_hash():
    try:
        r = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            capture_output=True, text=True, cwd=ROOT,
        )
        return r.stdout.strip() if r.returncode == 0 else "unknown"
    except Exception:
        return "unknown"


def git_commit_timestamp():
    try:
        r = subprocess.run(
            ["git", "log", "-1", "--format=%ct"],
            capture_output=True, text=True, cwd=ROOT,
        )
        return int(r.stdout.strip()) if r.returncode == 0 else 0
    except Exception:
        return 0


# ── Oracle Management ────────────────────────────────────────────────────


def oracle_image_name(project, version):
    """Docker image tag for a specific oracle version."""
    return f"libxml-rs/oracle:{project}-{version}"


def build_oracle(project, version):
    """Build a Docker oracle image for the given project + version.

    This uses the Dockerfile.oracle and the source tarball from the
    upstream GNOME download servers.
    """
    print(f"Building oracle: {project} {version}")

    # Map project to the correct source package
    if project == "libxml2":
        src_pkg = "libxml2"
    elif project == "libxslt":
        src_pkg = "libxslt"
    else:
        print(f"ERROR: unknown project: {project}", file=sys.stderr)
        sys.exit(1)

    image_tag = oracle_image_name(project, version)

    cmd = [
        "docker", "build",
        "-t", image_tag,
        "-f", str(ROOT / "docker" / "Dockerfile.oracle"),
        "--build-arg", f"{src_pkg.upper()}_VERSION={version}",
        str(ROOT),
    ]
    print(f"  Running: {' '.join(cmd)}")
    r = subprocess.run(cmd)
    if r.returncode != 0:
        print(f"ERROR: oracle build failed for {project} {version}", file=sys.stderr)
        sys.exit(1)

    print(f"  Oracle image built: {image_tag}")
    return image_tag


def run_in_oracle(image_tag, command, input_data=None, timeout=30):
    """Run a command inside the oracle Docker container.

    Returns (stdout_bytes, stderr_bytes, exit_status).
    """
    cmd = [
        "docker", "run", "--rm",
        "-i",  # interactive for stdin
        image_tag,
        "/bin/bash", "-c", command,
    ]
    r = subprocess.run(
        cmd,
        input=input_data,
        capture_output=True,
        timeout=timeout,
    )
    return r.stdout, r.stderr, r.returncode


# ── Candidate Management ─────────────────────────────────────────────────


def build_candidate():
    """Build libxml-rs artifacts (static lib, shared lib, bins)."""
    print("Building candidate: libxml-rs")
    cmd = ["cargo", "build", "--release"]
    r = subprocess.run(cmd, cwd=ROOT)
    if r.returncode != 0:
        print("ERROR: candidate build failed", file=sys.stderr)
        sys.exit(1)

    # Locate artifacts
    target_dir = ROOT / "target" / "release"
    artifacts = {}
    for name, ext in [
        ("libxml2_rs", "so"), ("libxml2_rs", "a"),
        ("xmllint", ""), ("xsltproc", ""), ("xmlcatalog", ""),
    ]:
        if ext:
            pattern = f"lib{name}.{ext}"
        else:
            pattern = name
        found = list(target_dir.glob(pattern))
        if found:
            artifacts[name] = found[0]

    print(f"  Candidate artifacts: {list(artifacts.keys())}")
    return artifacts


# ── Case Execution ───────────────────────────────────────────────────────


def run_case(casefile_path, oracle_image=None):
    """Execute a single case against oracle and/or candidate.

    Returns a receipt dict.
    """
    case = load_json(casefile_path)
    case_id = case["case_id"]
    print(f"Running case: {case_id}")

    receipt = {
        "case_id": case_id,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "candidate_commit": git_commit_hash(),
        "candidate_commit_time": git_commit_timestamp(),
        "oracle_image": oracle_image,
        "observations": {},
        "residuals": [],
        "verdict": "UNKNOWN",
    }

    # ── Run against oracle ────────────────────────────────────────────
    if oracle_image:
        oracle_obs = _run_oracle(case, oracle_image)
        receipt["observations"]["oracle"] = oracle_obs

    # ── Run against candidate ─────────────────────────────────────────
    candidate_obs = _run_candidate(case)
    receipt["observations"]["candidate"] = candidate_obs

    # ── Compare ───────────────────────────────────────────────────────
    if "oracle" in receipt["observations"] and "candidate" in receipt["observations"]:
        residuals = _compare(receipt["observations"]["oracle"],
                             receipt["observations"]["candidate"],
                             case.get("normalization", []))
        receipt["residuals"] = residuals
        receipt["verdict"] = "PASS" if not residuals else "RESIDUAL"

    # ── Write receipt ─────────────────────────────────────────────────
    receipt_path = RECEIPTS_DIR / case_id / f"{datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S')}.json"
    write_json(receipt_path, receipt)
    print(f"  Receipt: {receipt_path}")

    return receipt


def _run_oracle(case, image_tag):
    """Run a case against the oracle Docker image."""
    obs = {}

    # Determine what kind of test this is
    surface = case.get("surface", "")
    inputs = case.get("inputs", {})

    if surface.startswith("cli-"):
        # CLI case: run the tool inside the oracle container
        argv = inputs.get("argv", [])
        cmd = " ".join(argv)
        stdout, stderr, status = run_in_oracle(image_tag, cmd)
        obs["stdout"] = stdout.decode("utf-8", errors="replace")
        obs["stderr"] = stderr.decode("utf-8", errors="replace")
        obs["exit_status"] = status
        obs["stdout_hash"] = hash_bytes(stdout)
        obs["stderr_hash"] = hash_bytes(stderr)

    elif surface in ("parser", "sax", "tree", "xpath", "xslt-compile", "xslt-execute"):
        # API-level case: use a C harness or Python ctypes
        # For now, use xmllint/xsltproc as the oracle measurement tool
        doc = inputs.get("document", "")
        expr = inputs.get("expression", "")
        opts = inputs.get("options", {})

        if expr:
            # XPath evaluation via xmllint --shell or --xpath
            cmd = f"xmllint --xpath '{expr}' '{doc}' 2>&1 || true"
        elif doc.endswith(".xsl") or doc.endswith(".xslt"):
            # XSLT via xsltproc
            cmd = f"xsltproc '{doc}' '{inputs.get('document', '')}' 2>&1 || true"
        else:
            # Plain parse via xmllint
            cmd = f"xmllint '{doc}' 2>&1 || true"

        stdout, stderr, status = run_in_oracle(image_tag, cmd)
        obs["stdout"] = stdout.decode("utf-8", errors="replace")
        obs["stderr"] = stderr.decode("utf-8", errors="replace")
        obs["exit_status"] = status
        obs["stdout_hash"] = hash_bytes(stdout)
        obs["stderr_hash"] = hash_bytes(stderr)

    else:
        obs["error"] = f"Unsupported surface: {surface}"

    return obs


def _run_candidate(case):
    """Run a case against the candidate (libxml-rs) artifacts."""
    obs = {}
    surface = case.get("surface", "")
    inputs = case.get("inputs", {})

    # Determine which candidate binary to use
    if surface.startswith("cli-"):
        tool_name = surface.split("-", 1)[1]  # xmllint, xsltproc, xmlcatalog
        candidate_bin = ROOT / "target" / "release" / tool_name
        if not candidate_bin.exists():
            obs["error"] = f"Candidate binary not found: {candidate_bin}"
            return obs

        argv = inputs.get("argv", [])
        cmd = [str(candidate_bin)] + argv
        r = subprocess.run(cmd, capture_output=True, timeout=30)
        obs["stdout"] = r.stdout.decode("utf-8", errors="replace")
        obs["stderr"] = r.stderr.decode("utf-8", errors="replace")
        obs["exit_status"] = r.returncode
        obs["stdout_hash"] = hash_bytes(r.stdout)
        obs["stderr_hash"] = hash_bytes(r.stderr)

    else:
        obs["error"] = f"Unsupported surface: {surface}"

    return obs


# ── Comparison ───────────────────────────────────────────────────────────


def _compare(oracle_obs, candidate_obs, normalizations):
    """Compare oracle and candidate observations, returning residuals.

    A residual is any observable difference that cannot be explained by
    the declared normalization rules.
    """
    residuals = []

    # Build a set of normalized fields
    normalized_fields = {n["field"] for n in normalizations}

    # Compare exit status
    if "exit_status" in oracle_obs and "exit_status" in candidate_obs:
        if oracle_obs["exit_status"] != candidate_obs["exit_status"]:
            if "exit_status" not in normalized_fields:
                residuals.append({
                    "field": "exit_status",
                    "oracle": oracle_obs["exit_status"],
                    "candidate": candidate_obs["exit_status"],
                })

    # Compare stdout
    if "stdout_hash" in oracle_obs and "stdout_hash" in candidate_obs:
        if oracle_obs["stdout_hash"] != candidate_obs["stdout_hash"]:
            if "stdout" not in normalized_fields:
                residuals.append({
                    "field": "stdout",
                    "oracle_hash": oracle_obs["stdout_hash"],
                    "candidate_hash": candidate_obs["stdout_hash"],
                    "oracle_preview": oracle_obs.get("stdout", "")[:200],
                    "candidate_preview": candidate_obs.get("stdout", "")[:200],
                })

    # Compare stderr
    if "stderr_hash" in oracle_obs and "stderr_hash" in candidate_obs:
        if oracle_obs["stderr_hash"] != candidate_obs["stderr_hash"]:
            if "stderr" not in normalized_fields:
                residuals.append({
                    "field": "stderr",
                    "oracle_hash": oracle_obs["stderr_hash"],
                    "candidate_hash": candidate_obs["stderr_hash"],
                    "oracle_preview": oracle_obs.get("stderr", "")[:200],
                    "candidate_preview": candidate_obs.get("stderr", "")[:200],
                })

    return residuals


# ── CLI ──────────────────────────────────────────────────────────────────


def cmd_build_oracle(args):
    build_oracle(args.project, args.version)


def cmd_build_candidate(args):
    build_candidate()


def cmd_run(args):
    oracle_image = None
    if args.oracle:
        oracle_image = oracle_image_name(args.project, args.version)
    run_case(args.casefile, oracle_image)


def cmd_sweep(args):
    """Run all casefiles in a directory."""
    suite_dir = COURTS_DIR / "suites" / args.suite
    if not suite_dir.exists():
        print(f"ERROR: suite not found: {suite_dir}", file=sys.stderr)
        sys.exit(1)

    casefiles = sorted(suite_dir.glob("*.json"))
    print(f"Running suite '{args.suite}' ({len(casefiles)} cases)")

    oracle_image = None
    if args.oracle:
        oracle_image = oracle_image_name(args.project, args.version)

    results = {"pass": 0, "residual": 0, "error": 0}
    for cf in casefiles:
        try:
            receipt = run_case(cf, oracle_image)
            if receipt["verdict"] == "PASS":
                results["pass"] += 1
            elif receipt["verdict"] == "RESIDUAL":
                results["residual"] += 1
            else:
                results["error"] += 1
        except Exception as e:
            print(f"  ERROR: {cf.name}: {e}", file=sys.stderr)
            results["error"] += 1

    print(f"\nSuite results: {results['pass']} pass, "
          f"{results['residual']} residual, {results['error']} error")
    return results


def main():
    parser = argparse.ArgumentParser(
        description="libxml-rs Court Runner — Differential testing framework")
    sub = parser.add_subparsers(dest="command", help="Sub-command")

    # build-oracle
    bo = sub.add_parser("build-oracle", help="Build a Docker oracle image")
    bo.add_argument("project", choices=["libxml2", "libxslt"])
    bo.add_argument("version")
    bo.set_defaults(func=cmd_build_oracle)

    # build-candidate
    bc = sub.add_parser("build-candidate", help="Build libxml-rs artifacts")
    bc.set_defaults(func=cmd_build_candidate)

    # run
    run_p = sub.add_parser("run", help="Run a single case")
    run_p.add_argument("casefile", type=Path)
    run_p.add_argument("--oracle", action="store_true", help="Run against oracle")
    run_p.add_argument("--project", default="libxml2")
    run_p.add_argument("--version", default="2.15.3")
    run_p.set_defaults(func=cmd_run)

    # sweep
    sw = sub.add_parser("sweep", help="Run all cases in a suite")
    sw.add_argument("suite")
    sw.add_argument("--oracle", action="store_true", help="Run against oracle")
    sw.add_argument("--project", default="libxml2")
    sw.add_argument("--version", default="2.15.3")
    sw.set_defaults(func=cmd_sweep)

    args = parser.parse_args()
    if args.command is None:
        parser.print_help()
        sys.exit(1)
    args.func(args)


if __name__ == "__main__":
    main()
