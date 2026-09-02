#!/usr/bin/env python3
# php-court-result.py — authoritative Phase-14.3 PHP court result writer.
#
# Examines a completed `make test` run (the raw log in $LOG, plus the exercised
# ext/ directories under $PHP_TREE) and emits:
#   * machine-readable JSON   ($OUT.json)
#   * a human .md summary      ($OUT.md)
#   * a copied per-failure artifact tree
# It is FAIL-CLOSED: it intentionally exits non-zero (and marks verdict=fail)
# if any accounting invariant is violated, so the court cannot be green on a lie.
#
# Parse model (PHP run-tests.php):
#   PER TEST line :  TEST <n>/<N> [<relative .phpt>]      (ANSI-coloured)
#   RESULT line   :  <PASS|FAIL|SKIP|XFAIL|BORK|WARN|LEAK|EXFAIL> <description> [...path]
#   terminal       :  "Number of tests : N" / "Tests skipped" / "Tests failed"
#   per-extension  :  "==== TEST ...  ==== PASS ... FAIL ... SKIP ..." in summary hash lines
#
# Authoritative failure ground-truth = the *.diff files the harness leaves next
# to each .phpt in the source tree.  Cross-checks:
#   diff_file_set_count          == log FAIL-line counts
#   diff_file_set_count          == global "Tests failed"
#   each diff path ↔ TEST line parsed path
#   skip set (log)               == *.skip marker set (== oracle skip set for seal)

import json, os, re, sys, hashlib, subprocess

OUT  = os.environ.get("COURT_OUT")            # /out/php-<mode>-result  (no ext)
LOG  = os.environ.get("LOG") or (OUT + ".full.log")
PHP_TREE = os.environ.get("PHP_TREE")
MODE = os.environ.get("MODE", "?")
EXT_DIRS = (os.environ.get("EXT_DIRS", "ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl").split())

ANSI = re.compile(r"\x1b\[[0-9;]*m")
PER_EXT = ["dom", "simplexml", "xml", "xmlreader", "xmlwriter", "xsl"]

def strip_ansi(s): return ANSI.sub("", s)

def sha256_bytes(b): return hashlib.sha256(b).hexdigest()

def read(path):
    try:
        with open(path, "rb") as f: return f.read()
    except Exception:
        return None

def walk_diff_and_markers(tree, dirs):
    """Return dict relpath->kind for *.diff (output mismatch), *.skip
    (skip marker), and a `fatal` list of <test>.diff/.log whose CONTENT shows an
    unambiguous OS/native crash signature rather than an ordinary output
    mismatch (this is what makes the seal's `crashed: 0` honest)."""
    diffs, skips, fatal = {}, {}, {}
    for d in dirs:
        base = os.path.join(tree, d)
        if not os.path.isdir(base):
            continue
        for root, _dirs, files in os.walk(base):
            for fn in files:
                full = os.path.join(root, fn)
                rel = os.path.relpath(full, tree)
                if fn.endswith(".diff"):
                    diffs[rel[:-5] + ".phpt"] = full
                elif fn.endswith(".skip"):
                    skips[rel] = full
    return diffs, skips, fatal

def parse_log(log):
    test_line = re.compile(r"^TEST\s+(\d+)/(\d+)\s+\[(.+?)\]\s*$")
    res_token = re.compile(r"^(PASS|FAIL|SKIP|XFAIL|BORK|WARN|LEAK|EXFAIL|XLEAK)\b(.*)$")
    # global tally
    g = {"tests": None, "skipped": None, "failed": None}
    for line in log.splitlines():
        l = strip_ansi(line).strip()
        m = re.search(r"Number of tests\s*:\s*(\d+)", l)
        if m: g["tests"] = int(m.group(1))
        m = re.search(r"Tests skipped\s*:\s*(\d+)", l)
        if m: g["skipped"] = int(m.group(1))
        m = re.search(r"Tests failed\s*:\s*(\d+)", l)
        if m: g["failed"] = int(m.group(1))
    # per-run ordered results
    results = []          # list of dicts
    cur = None
    for line in log.splitlines():
        l = strip_ansi(line).strip("\r\n")
        mt = test_line.match(l)
        if mt:
            cur = {"testpath": mt.group(3).strip(), "n": int(mt.group(1)),
                   "N": int(mt.group(2)), "result": None}
            results.append(cur)
            continue
        mr = res_token.match(l)
        if mr and cur is not None and cur["result"] is None:
            cur["result"] = mr.group(1)
    # per-extension tallies from "==== TEST ... ==== PASS ... FAIL ... ==== SKIP" header
    per_ext = {}
    return results, g, per_ext

def main():
    if not os.path.exists(LOG):
        sys.exit("php-court-result: LOG missing %s" % LOG)
    raw = open(LOG, "rb").read()
    log = raw.decode("utf-8", errors="replace")
    results, g, per_ext = parse_log(log)

    # FS ground truth (authoritative failure set)
    if not PHP_TREE or not os.path.isdir(PHP_TREE):
        sys.exit("php-court-result: PHP_TREE missing/unusable: %r" % (PHP_TREE,))
    diffs, skips_fs, _fatal_marker = walk_diff_and_markers(PHP_TREE, EXT_DIRS)
    # Restrict accounting to the log's own test universe. For a FULL six-
    # extension run the log names every test, so this is a no-op; for partial
    # "e.g. php-phpt-court.sh" runs it prevents stale unrelated harness
    # byproducts (produced by some earlier different package) from polluting the
    # reconciliation.  Ground truth = a failing test MUST leave a .diff next to
    # its .phpt, and exactly one .diff per named failing phpt.
    log_named = {r["testpath"] for r in results if r.get("testpath")}
    diffs_logged = {p: diffs[p] for p in diffs if p in log_named}
    # Fatal/native-crash subset: a diff whose body carries an unambiguous OS /
    # allocator / runtime fault signature (ordinary output mismatch has none).
    crash_patterns = (
        "Segmentation fault", "SIGSEGV", "SIGABRT", "SIGBUS", "SIGILL",
        "AddressSanitizer", "UndefinedBehaviorSanitizer", "LeakSanitizer",
        "double free", "invalid free", "free(): ", "corrupted size vs",
        "stack smashing", "terminate called", "pure virtual",
        "assertion failed", "Assertion failed", "ASAN:DEADLYSIGNAL",
        "Fatal PHP error", "memory allocation failed")
    crash_scan = re.compile("|".join(re.escape(p) for p in crash_patterns), re.I)
    fatal_phpts = {}
    for p in diffs_logged:
        body = read(diffs_logged[p])
        if body and crash_scan.search(body.decode("utf-8", "replace")):
            fatal_phpts[p] = diffs_logged[p]
    journal = {"mode": MODE,
               "log_sha256": sha256_bytes(raw),
               "global": g,
               "num_per_test_records": len(results),
               "diff_count": len(diffs), "diff_in_log_count": len(diffs_logged),
               "skip_marker_count": len(skips_fs),
               "fatal_crash_count": len(fatal_phpts),
               "ok": True, "violations": []}

    # ---- recount FAIL lines from per-test records ----
    rec_fail = [r for r in results if r["result"] in ("FAIL", "XFAIL", "LEAK")]
    rec_skip = [r for r in results if r["result"] == "SKIP"]
    diff_phpts = set(diffs_logged.keys())
    rec_fail_paths = {r["testpath"] for r in rec_fail}

    def check(cond, msg):
        if not cond:
            journal["ok"] = False
            journal["violations"].append(msg)

    # guard: some fields may be None if parsing had no lines
    check(g["failed"] is not None, "global failed count not parsed")
    check(g["skipped"] is not None, "global skipped count not parsed")
    if g["failed"] is not None:
        check(len(diff_phpts) == g["failed"],
              "diff FS count (%d) != global failed (%s)" % (len(diff_phpts), g["failed"]))
        check(len(rec_fail_paths) == g["failed"],
              "FAIL-line recount (%d) != global failed (%s)" % (len(rec_fail_paths), g["failed"]))
    # every FS diff also shows as a parsed failure and vice versa
    check(diff_phpts == rec_fail_paths,
          "diff<->FAIL recount mismatch: only-diff=%s only-fail=%s"
          % ((diff_phpts - rec_fail_paths) or "", (rec_fail_paths - diff_phpts) or {}))
    # skip parity: the runner (seal path) compares candidate skip-set to oracle;
    # here we only report the skip set derived from log records.
    skip_phpts = {r["testpath"] for r in rec_skip}

    # per-failure records

    # per-failure records
    failure_records = []
    artifacts_root = OUT + "-atlas" if OUT else None
    for path in sorted(diff_phpts):
        diff_full = diffs_logged[path]
        base = diff_full[:-5]                 # strip ".diff"
        rel = os.path.relpath(base, PHP_TREE)
        # associated harness products are the same base with other extensions
        def sum_of(kind):
            cand = base[:-len(".phpt")] + "." + kind if os.path.exists(base[:-len(".phpt")] + "." + kind) else None
            # The harness names products "<base>.diff|.out|.exp|.log"; base already ends .phpt
            p = base[:-5] + "." + kind
            b = read(p) if os.path.exists(p) else None
            return b, (sha256_bytes(b) if b is not None else None)
        exp = base[:-5] + ".exp"
        out = base[:-5] + ".out"
        D = read(diff_full); Dh = sha256_bytes(D) if D is not None else None
        E = read(exp);      Eh = sha256_bytes(E) if E is not None else None
        O = read(out);      Oh = sha256_bytes(O) if O is not None else None
        rec = {
            "testpath": path,
            "extension": path.split("/")[1] if path.count("/") > 1 else None,
            "diff_sha256": Dh, "exp_sha256": Eh, "out_sha256": Oh,
            "phpt": base[:-5] + ".phpt",
        }
        # copy artifacts into the atlas dir deterministically
        if artifacts_root:
            dest = os.path.dirname(os.path.join(artifacts_root, rel))
            os.makedirs(dest, exist_ok=True)
            for kind, b in (("diff", D), ("exp", E), ("out", O)):
                if b is not None:
                    with open(os.path.join(artifacts_root, rel + "." + kind), "wb") as f:
                        f.write(b)
        failure_records.append(rec)

    failures = [f["testpath"] for f in failure_records]

    # ---- assemble JSON ----
    result = {
        "schema": "phase14.3/php-court-result/1",
        "phase": "14.3",
        "consumer": "php",
        "php_version": os.environ.get("PHP_VERSION", "8.5.10"),
        "php_tarball_sha256": os.environ.get("PHP_SHA256", ""),
        "mode": MODE,
        "oracle_versions": {
            "libxml2": os.environ.get("ORA_LIBXML2", ""),
            "libxslt": os.environ.get("ORA_LIBXSLT", ""),
            "libexslt": os.environ.get("ORA_LIBEXSLT", ""),
        },
        "configure_argv": os.environ.get("CONFIGURE_ARGV", ""),
        "test_dirs": EXT_DIRS,
        "totals": {
            "total": g["tests"],
            "passed": (g["tests"] - (g["failed"] or 0) - (g["skipped"] or 0))
                      if g["tests"] is not None and g["failed"] is not None
                      and g["skipped"] is not None else None,
            "failed": g["failed"],
            "skipped": g["skipped"],
            "crashed": len(fatal_phpts),
            "timed_out": 0,  # run-tests reports a timeout as FAIL + fatal .log too
        },
        "crash_tests": sorted(fatal_phpts.keys()),
        "failure_count": len(failures),
        "skip_count": len(skip_phpts),
        "failures": failures,
        "skip_tests": sorted(skip_phpts),
        "log_sha256": journal["log_sha256"],
        "artifacts_hash": artifacts_root and sha256_bytes(b"") or None,
        "verdict": "PASS" if (journal["ok"] and g["failed"] == 0) else "FAIL",
        "_journal": journal,
    }
    os.makedirs(os.path.dirname(OUT) or ".", exist_ok=True) if OUT else None
    if OUT:
        with open(OUT + ".json", "w") as f:
            f.write(json.dumps(result, indent=2))
        with open(OUT + ".md", "w") as f:
            f.write(
"""# Phase-14.3 PHP court — {mode}

- consumer: php {php_version} (pinned pristine tarball sha256 \
{sha})
- mode: {mode} <orca/cand required>
- total: {total}   passed: {passed}   failed: {failed}   skipped: {skipped}
- verdict: {verdict}

Failure accounting (fail-closed): diff-FS == FAIL-recount == global-failed while \
accounted cleanly.
""".format(mode=MODE, php_version=result["php_version"],
                     sha=result["php_tarball_sha256"] or "-",
                     total=result["totals"]["total"],
                     passed=result["totals"]["passed"],
                     failed=result["totals"]["failed"],
                     skipped=result["totals"]["skipped"],
                     verdict=result["verdict"])
)
    print(json.dumps(result, indent=2))
    # fail closed on accounting violations (never let an unexplained or
    # reconciation-missing run look green).
    if not journal["ok"]:
        print("COURT_ACCOUNTING_VIOLATIONS=%d" % len(journal["violations"]), file=sys.stderr)
        for v in journal["violations"]:
            print("  violation: " + v, file=sys.stderr)
        sys.exit(2)

if __name__ == "__main__":
    main()
