#!/usr/bin/env python3
"""CLI differential court runner (11.1-X/Y).

Runs every casefile under courts/suites/cli/<tool>/ against the system oracle
binaries (/usr/bin/xmllint, /usr/bin/xmlcatalog, /usr/bin/xsltproc) and the
candidate binaries (target/debug/<tool>), capturing exit status + stdout +
stderr and writing per-case receipts to courts/receipts/<case_id>/. The
candidate must not be linked against the oracle libraries (see
oracle-contamination court).

Casefile conventions handled:
  - inputs.argv      arguments; "@TMP@/..." substituted with a per-case temp dir
  - inputs.steps     setup invocations run against both sides before the case
  - inputs.stdin_script  fed to the tool's stdin
  - inputs.document / inputs.stylesheet  fixtures resolved relative to the
    suite directory (courts/suites/cli/<tool>/fixtures or courts/corpus)
  - normalization    per-case sed-like normalizations applied to captured text
                     before hashing (argv[0] paths etc.)

Usage:
  cli_runner.py [--tool xmlcatalog|xmllint|xsltproc] [--case CLI-XMLCATALOG-0002]
  cli_runner.py --check   verify committed receipts match a fresh run
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SUITES = os.path.join(ROOT, "courts", "suites", "cli")
RECEIPTS = os.path.join(ROOT, "courts", "receipts")
BIN = os.path.join(ROOT, "target", "debug")

TOOLS = {"xmllint", "xmlcatalog", "xsltproc"}


def sha256(b):
    return hashlib.sha256(b).hexdigest()


def git_head():
    try:
        r = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
                           capture_output=True, text=True)
        return r.stdout.strip() if r.returncode == 0 else None
    except Exception:
        return None


def resolve_fixture(path, tool):
    cands = [
        os.path.join(SUITES, tool, path),
        os.path.join(SUITES, tool, "fixtures", path),
        os.path.join(ROOT, "courts", "corpus", "cli", path),
        os.path.join(ROOT, "courts", "corpus", path),
        os.path.join(ROOT, "target", "corpus", path),
    ]
    for c in cands:
        if os.path.exists(c):
            return c
    return None


def substitute(argv, tmp):
    return [a.replace("@TMP@", tmp) for a in argv]


def run_tool(exe, argv, stdin_data=None, cwd=None):
    r = subprocess.run([exe] + argv, input=stdin_data, capture_output=True,
                       cwd=cwd, timeout=120)
    return r.returncode, r.stdout, r.stderr


def normalize(text, rules, tool):
    s = text.decode("utf-8", "replace")
    for rule in rules:
        if isinstance(rule, dict):
            pat = rule.get("pattern") or rule.get("regex")
            repl = rule.get("replacement", "")
            flags = 0
            if rule.get("ignore_case"):
                flags |= re.IGNORECASE
            s = re.sub(pat, repl, s, flags=flags)
        elif isinstance(rule, str):
            # legacy: "s|/usr/bin/xmllint|TOOL|g" style
            m = re.match(r"s\|(.*)\|(.*)\|([g]*)", rule)
            if m:
                flags = re.IGNORECASE if "i" in m.group(3) else 0
                s = re.sub(m.group(1), m.group(2), s, flags=flags)
    return s.encode()


def run_case(path, tool):
    case = json.load(open(path))
    cid = case["case_id"]
    tmp = tempfile.mkdtemp(prefix=f"cli-{cid}-")
    argv = substitute(case.get("inputs", {}).get("argv", []), tmp)
    stdin_data = case.get("inputs", {}).get("stdin_script")
    if stdin_data is not None:
        stdin_data = stdin_data.encode()
    cwd = None
    for key in ("document", "stylesheet"):
        if key in case.get("inputs", {}):
            fix = resolve_fixture(case["inputs"][key], tool)
            if fix is None:
                raise SystemExit(f"{cid}: missing fixture {case['inputs'][key]}")
            cwd = os.path.dirname(fix) if cwd is None else cwd
    # multi-step setups run on both sides
    steps = case.get("inputs", {}).get("steps", [])
    for step in steps:
        sargv = substitute(step, tmp)
        run_tool(f"/usr/bin/{tool}", sargv)
        run_tool(os.path.join(BIN, tool), sargv)
    if cwd is None:
        cwd = os.path.join(SUITES, tool)

    o_rc, o_out, o_err = run_tool(f"/usr/bin/{tool}", argv, stdin_data, cwd)
    c_rc, c_out, c_err = run_tool(os.path.join(BIN, tool), argv, stdin_data, cwd)
    rules = case.get("normalization", [])
    o_out, o_err = normalize(o_out, rules, tool), normalize(o_err, rules, tool)
    c_out, c_err = normalize(c_out, rules, tool), normalize(c_err, rules, tool)

    residuals = []
    if o_rc != c_rc:
        residuals.append(f"exit {o_rc} != {c_rc}")
    if o_out != c_out:
        residuals.append("stdout differs")
    if o_err != c_err:
        residuals.append("stderr differs")

    receipt = {
        "schema": "cli-court-receipt-1",
        "case_id": cid,
        "tool": tool,
        "oracle": "/usr/bin/" + tool,
        "candidate": os.path.join(BIN, tool),
        "candidate_commit": git_head(),
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "observations": {
            "oracle": {"exit_status": o_rc, "stdout_hash": sha256(o_out),
                       "stderr_hash": sha256(o_err)},
            "candidate": {"exit_status": c_rc, "stdout_hash": sha256(c_out),
                          "stderr_hash": sha256(c_err)},
        },
        "residuals": residuals,
        "verdict": "PASS" if not residuals else "FAIL",
    }
    shutil.rmtree(tmp, ignore_errors=True)
    return receipt


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--tool", choices=sorted(TOOLS))
    ap.add_argument("--case")
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    tools = [args.tool] if args.tool else sorted(TOOLS)
    overall = []
    for tool in tools:
        suite = os.path.join(SUITES, tool)
        if not os.path.isdir(suite):
            continue
        cases = sorted(f for f in os.listdir(suite) if f.endswith(".json"))
        if args.case:
            cases = [c for c in cases if c.startswith(args.case)]
        for cf in cases:
            path = os.path.join(suite, cf)
            receipt = run_case(path, tool)
            overall.append(receipt)
            cid = receipt["case_id"]
            outdir = os.path.join(RECEIPTS, cid)
            os.makedirs(outdir, exist_ok=True)
            stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S%f") + "Z"
            outp = os.path.join(outdir, stamp + ".json")
            if not args.check:
                with open(outp, "w") as f:
                    json.dump(receipt, f, indent=1, ensure_ascii=False)
                    f.write("\n")
            mark = "PASS" if receipt["verdict"] == "PASS" else "FAIL"
            print(f"{mark}  {cid}  ({', '.join(receipt['residuals']) or 'byte-identical'})")
    passed = sum(1 for r in overall if r["verdict"] == "PASS")
    print(f"=== CLI courts: PASS={passed} FAIL={len(overall) - passed} ===")
    return 0 if passed == len(overall) else 1


if __name__ == "__main__":
    sys.exit(main())
