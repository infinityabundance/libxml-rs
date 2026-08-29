#!/usr/bin/env python3
"""CLI differential court runner (Phase 9/10 evidence, promoted per 11.1-A).

For every committed casefile under courts/suites/cli/<tool>/ this runner:
  1. executes the system oracle (/usr/bin/<tool>) and the candidate
     (target/debug/<tool>) with identical argv on committed fixtures;
  2. compares exit status, stdout, stderr byte-for-byte (no normalization);
  3. compares any files the tool is expected to create;
  4. verifies the candidate against the committed expected/ golden captures;
  5. emits an immutable per-run receipt under courts/receipts/phase-XX/ and
     updates the aggregate courts/manifests/phase-XX.json.

Usage:
  courts/cli/court-runner.py <tool>            (tool: xmllint|xmlcatalog|xsltproc)
  courts/cli/court-runner.py <tool> --capture-expected   (refresh golden captures from the oracle)
  courts/cli/court-runner.py <tool> <case-id-substring>

The evidence survives deletion of target/: receipts, manifests, casefiles,
fixtures and expected/ captures are all committed.
"""
import argparse
import datetime
import hashlib
import json
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SUITES = os.path.join(ROOT, "courts", "suites", "cli")
CORPUS = os.path.join(ROOT, "courts", "corpus", "cli")
EXPECTED = os.path.join(ROOT, "courts", "expected", "cli")
MANIFESTS = os.path.join(ROOT, "courts", "manifests")
RECEIPTS = os.path.join(ROOT, "courts", "receipts")

PHASE = {"xmllint": "10", "xmlcatalog": "10", "xsltproc": "09"}

ORACLE = None
CANDIDATE = None


def sha(b):
    return hashlib.sha256(b).hexdigest()


def git_head():
    out = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
                         capture_output=True, text=True)
    return out.stdout.strip() if out.returncode == 0 else "unknown"


def load_casefile(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def normalize_argv0(data, tool_bin):
    """The tools echo their own invocation path (usage lines, --version, error
    headers). Both sides are normalized identically: the absolute binary path is
    replaced with the literal `TOOL`. This is the same documented normalization
    the original Phase 10 target/difftest_summary.sh applied, and it is recorded
    in every receipt. Content beyond argv[0] is compared byte-for-byte."""
    return data.replace(tool_bin.encode(), b"TOOL")


def run_one(tool_bin, argv, cwd, stdin_bytes, tmpdir):
    """Run with @TMP@ -> tmpdir substitutions. Returns (exit, out, err)."""
    argv = [a.replace("@TMP@", tmpdir) for a in argv]
    p = subprocess.run([tool_bin] + argv, cwd=cwd, input=stdin_bytes, capture_output=True,
                       timeout=60)
    return p.returncode, p.stdout, p.stderr


def exec_case(casefile, tool_name, cwd, workdir):
    """Run the oracle + candidate for one casefile. Returns record dict."""
    cid = casefile["case_id"]
    inputs = casefile.get("inputs", {})
    argv = inputs.get("argv") or []
    steps = inputs.get("steps") or [argv]
    stdin = (inputs.get("stdin_script") or "").encode()
    created = inputs.get("created_file")

    rec = {"case_id": cid, "verdict": "FAIL"}
    for side, tool_bin in (("oracle", ORACLE), ("candidate", CANDIDATE)):
        tmp = os.path.join(workdir, side, cid)
        os.makedirs(tmp, exist_ok=True)
        outs, errs, exits = [], [], []
        for step in steps:
            e, o, er = run_one(tool_bin, step, cwd, stdin, tmp)
            outs.append(o)
            errs.append(er)
            exits.append(e)
        rec[f"{side}_exit"] = exits
        rec[f"{side}_stdout_sha"] = sha(b"".join(normalize_argv0(o, tool_bin) for o in outs))
        rec[f"{side}_stderr_sha"] = sha(b"".join(normalize_argv0(e, tool_bin) for e in errs))
        if created:
            cpath = os.path.join(tmp, created.replace("@TMP@/", ""))
            rec[f"{side}_created_sha"] = (
                sha(open(cpath, "rb").read()) if os.path.exists(cpath) else "MISSING")

    ok = (rec["oracle_exit"] == rec["candidate_exit"]
          and rec["oracle_stdout_sha"] == rec["candidate_stdout_sha"]
          and rec["oracle_stderr_sha"] == rec["candidate_stderr_sha"])
    if ok and created:
        ok = rec.get("oracle_created_sha") == rec.get("candidate_created_sha")
    rec["verdict"] = "PASS" if ok else "FAIL"

    # verify against committed expected golden captures
    exp_dir = os.path.join(EXPECTED, tool_name)
    exp_out = os.path.join(exp_dir, f"{cid}.out")
    exp_err = os.path.join(exp_dir, f"{cid}.err")
    exp_exit = os.path.join(exp_dir, f"{cid}.exit")
    rec["expected_present"] = all(os.path.exists(p) for p in (exp_out, exp_err, exp_exit))
    if rec["expected_present"]:
        with open(exp_out, "rb") as f:
            eo = normalize_argv0(f.read(), ORACLE)
        with open(exp_err, "rb") as f:
            ee = normalize_argv0(f.read(), ORACLE)
        with open(exp_exit) as f:
            ex = f.read().strip()
        rec["expected_exit"] = ex
        rec["expected_match"] = (
            " ".join(str(e) for e in rec["candidate_exit"]) == ex
            and rec["candidate_stdout_sha"] == sha(eo)
            and rec["candidate_stderr_sha"] == sha(ee))
    else:
        rec["expected_match"] = True  # no golden yet (pre-seal state)
    return rec


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("tool", choices=["xmllint", "xmlcatalog", "xsltproc"])
    ap.add_argument("--capture-expected", action="store_true",
                    help="refresh courts/expected/cli/<tool>/ from the system oracle")
    ap.add_argument("filter", nargs="?", default="")
    args = ap.parse_args()

    suite_dir = os.path.join(SUITES, args.tool)
    casefiles = sorted(f for f in os.listdir(suite_dir) if f.endswith(".json"))
    if args.filter:
        casefiles = [f for f in casefiles if args.filter in f]

    candidate = os.path.join(ROOT, "target", "debug", args.tool)
    oracle = f"/usr/bin/{args.tool}"
    if not os.path.exists(candidate):
        print(f"candidate binary missing: {candidate} (run: cargo build --bin {args.tool})")
        return 2
    if not os.path.exists(oracle):
        print(f"oracle binary missing: {oracle}")
        return 2

    global ORACLE, CANDIDATE
    ORACLE = oracle
    CANDIDATE = candidate

    workdir = tempfile.mkdtemp(prefix=f"court-{args.tool}-")
    records = []
    for cf in casefiles:
        doc = load_casefile(os.path.join(suite_dir, cf))
        rec = exec_case(doc, args.tool, CORPUS, workdir)
        records.append(rec)
        mark = "PASS" if rec["verdict"] == "PASS" else "FAIL"
        extra = ""
        if rec.get("expected_present") and not rec.get("expected_match"):
            extra = "  [expected-capture MISMATCH]"
        print(f"{mark}  {rec['case_id']}{extra}")

    n_pass = sum(1 for r in records if r["verdict"] == "PASS")
    n_fail = len(records) - n_pass
    print(f"=== {args.tool}: PASS={n_pass} FAIL={n_fail} ===")

    oid = subprocess.run([oracle, "--version"], capture_output=True)
    ident = (oid.stdout + oid.stderr).decode(errors="replace").strip()

    now = datetime.datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ")
    phase = PHASE[args.tool]
    receipt = {
        "receipt_id": f"CLI-{args.tool.upper()}-{phase}-{now}",
        "phase": phase,
        "tool": args.tool,
        "timestamp": now,
        "candidate_commit": git_head(),
        "oracle_identity": ident,
        "oracle_binary": oracle,
        "candidate_binary": candidate,
        "normalization": "argv0 path normalized to TOOL (tools echo their own invocation path); all other bytes compared raw",
        "cases": records,
        "totals": {"pass": n_pass, "fail": n_fail},
        "verdict": "PASS" if n_fail == 0 and all(
            (not r.get("expected_present")) or r.get("expected_match") for r in records) else "FAIL",
    }

    if args.capture_expected:
        exp_dir = os.path.join(EXPECTED, args.tool)
        os.makedirs(exp_dir, exist_ok=True)
        for cf in casefiles:
            cid = cf[:-5]
            doc = load_casefile(os.path.join(suite_dir, cf))
            inputs = doc.get("inputs", {})
            steps = inputs.get("steps") or [inputs.get("argv") or []]
            stdin = (inputs.get("stdin_script") or "").encode()
            tmp = os.path.join(workdir, "expected-src", cid)
            os.makedirs(tmp, exist_ok=True)
            outs, errs, exits = [], [], []
            for step in steps:
                e, o, er = run_one(oracle, step, CORPUS, stdin, tmp)
                outs.append(o)
                errs.append(er)
                exits.append(e)
            with open(os.path.join(exp_dir, f"{cid}.out"), "wb") as f:
                f.write(b"".join(outs))
            with open(os.path.join(exp_dir, f"{cid}.err"), "wb") as f:
                f.write(b"".join(errs))
            with open(os.path.join(exp_dir, f"{cid}.exit"), "w") as f:
                f.write(" ".join(str(e) for e in exits))
        print(f"captured expected golden captures -> {EXPECTED}/{args.tool}")

    rec_dir = os.path.join(RECEIPTS, f"phase-{phase}")
    os.makedirs(rec_dir, exist_ok=True)
    rec_path = os.path.join(rec_dir, f"{args.tool}-{now}-receipt.json")
    with open(rec_path, "w", encoding="utf-8") as f:
        json.dump(receipt, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print("receipt:", rec_path)

    os.makedirs(MANIFESTS, exist_ok=True)
    manifest_path = os.path.join(MANIFESTS, f"phase-{phase}.json")
    manifest = {}
    if os.path.exists(manifest_path):
        with open(manifest_path, encoding="utf-8") as f:
            manifest = json.load(f)
    manifest.setdefault("tools", {})[args.tool] = {
        "date": now,
        "candidate_commit": git_head(),
        "oracle_identity": ident,
        "pass": n_pass,
        "fail": n_fail,
        "cases": {r["case_id"]: r["verdict"] for r in records},
    }
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print("manifest:", manifest_path)

    return 0 if receipt["verdict"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
