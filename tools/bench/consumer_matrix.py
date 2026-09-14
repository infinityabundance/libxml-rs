#!/usr/bin/env python3
"""consumer_matrix.py — §16.12 five-consumer performance orchestrator.

Reads the frozen §16.11 eligibility matrix and the corpus manifest, then for
each eligible (file, consumer, operation) runs the fixed consumer driver against
BOTH providers (upstream oracle and the libxml-rs candidate) inside the perf
container, measures wall time and max RSS, computes result equivalence (§16.14),
and aggregates per the frozen §16.11 aggregation contract.

Raw per-cell results are written to
`courts/receipts/phase-16/raw/16-12/<provider>/<consumer>/<id>.json`;
the combined matrix + aggregation to
`courts/receipts/phase-16/16-12-consumer-matrix.json`.

Usage:
  python3 tools/bench/consumer_matrix.py --tier1
  python3 tools/bench/consumer_matrix.py --all
  python3 tools/bench/consumer_matrix.py --all --consumers xmllint,python3-lxml
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MANIFEST = os.path.join(ROOT, "tools", "bench", "corpus", "manifest.json")
REPORT = os.path.join(ROOT, "courts", "receipts", "phase-16", "corpus-report.json")
ELIG = os.path.join(ROOT, "courts", "receipts", "phase-16", "corpus-eligibility.json")
RAW = os.path.join(ROOT, "courts", "receipts", "phase-16", "raw", "16-12")
OUT = os.path.join(ROOT, "courts", "receipts", "phase-16", "16-12-consumer-matrix.json")
CONTAINER = os.environ.get("PERF_CONTAINER", "perf-c")
CORPUS_DIR = os.environ.get("CORPUS_IN_CONTAINER", "/corpus")

CONSUMER_DRIVER = {
    "xmllint": "python3 /bench/cli_driver.py --consumer xmllint",
    "xsltproc": "python3 /bench/cli_driver.py --consumer xsltproc",
    "python3-lxml": "python3 /bench/lxml_driver.py",
    "ruby-nokogiri": "ruby /bench/nokogiri_driver.rb",
    "php": {"oracle": "/out/php-oracle -n /bench/php_driver.php",
            "candidate": "/out/php -n /bench/php_driver.php"},
}


def driver_cmd(consumer, mode):
    d = CONSUMER_DRIVER[consumer]
    return d[mode] if isinstance(d, dict) else d

BUCKET_ORDER = ["t0_lt_16k", "t1_16k_256k", "t2_256k_4m", "t3_4m_32m",
                "t4_32m_256m", "t5_gt_256m"]

TIME_RE = re.compile(r"TIME_S\s+([\d.]+)\s+RSS_KB\s+(\d+)")


def adaptive(args, nbytes):
    if args.reps is not None:
        r = args.reps
        return (r, max(1, r // 2))
    if nbytes <= 256 * 1024:
        return (5, 2)
    if nbytes <= 4 * 1024 * 1024:
        return (3, 1)
    return (1, 0)


def provider_env(mode: str) -> str:
    parts = ["source /court/consumers/lib.sh %s" % mode]
    if mode == "oracle":
        parts.append("export XSLTPERF=/out/xsltperf-oracle")
        parts.append("export PYTHONPATH=/out/lxml-oracle/src")
        parts.append("export SCHEMA_DIR=/bench/schema")
    else:
        parts.append("export XSLTPERF=/out/xsltperf-candidate")
        parts.append("export PYTHONPATH=/out/lxml-candidate/src")
        parts.append("export SCHEMA_DIR=/bench/schema")
    return "; ".join(parts)


def run_cell(mode, consumer, eid, category, path, ops, reps, warmup, timeout):
    driver = driver_cmd(consumer, mode)
    inner = (provider_env(mode) + "; export OP_TIMEOUT=%d; timeout -s KILL %d "
             % (max(5, timeout - 3), timeout)
             + "python3 /bench/runwrap.py -- "
             + driver
             + " --id %s --category %s --file %s --reps %d --warmup %d --ops %s"
             % (eid, category, path, reps, warmup, ",".join(ops)))
    cmd = ["docker", "exec", CONTAINER, "bash", "-lc", inner]
    t0 = time.perf_counter()
    try:
        p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                           timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"error": "timeout after %ss" % timeout, "ops": {}}
    wall = (time.perf_counter() - t0) * 1000.0
    out = p.stdout.decode("utf-8", "replace").strip().splitlines()
    err = p.stderr.decode("utf-8", "replace")
    data = {}
    if out:
        try:
            data = json.loads(out[-1])
        except Exception:  # noqa: BLE001
            data = {"error": "unparseable driver output", "raw": out[-1][:300]}
    m = TIME_RE.search(err)
    if m:
        data["process_s"] = float(m.group(1))
        data["rss_kb"] = int(m.group(2))
    data["docker_wall_ms"] = wall
    gc = re.search(r"GC_STAT (\{.*\})", err)
    if gc:
        try:
            data["gc_stat"] = json.loads(gc.group(1))
        except Exception:  # noqa: BLE001
            pass
    if p.returncode != 0 and "error" not in data:
        data["error"] = "driver rc=%d: %s" % (p.returncode, err[-300:])
    return data


def op_budget(nbytes):
    """Per-(file,consumer,operation) wall budget in seconds, sized so that a
    correct parser of this input size completes comfortably; a candidate that
    exceeds it is recorded as a timeout, not silently skipped. Fixed before the
    run (no candidate result influences it)."""
    return max(15, min(90, int(nbytes / 20_000_000)))


def cell_path(mode, consumer, eid, op):
    return os.path.join(RAW, mode, consumer, "%s__%s.json" % (eid, op))


def select_ids(entries, args):
    if args.ids:
        want = set(args.ids.split(","))
        return [e for e in entries if e["id"] in want]
    if args.tier1:
        bycat = {}
        for e in entries:
            bycat.setdefault(e["category"], []).append(e)
        sel = [min(v, key=lambda x: x["bytes"]) for v in bycat.values()]
        return sorted(sel, key=lambda x: x["id"])
    return entries


def _byte_identical(oo, cc):
    def sha_of(x):
        d = x.get("detail") or ""
        return d.split("out_sha=")[-1] if "out_sha=" in d else None
    a, b = sha_of(oo), sha_of(cc)
    return (a == b) if (a and b) else None


def aggregate(cells):
    """candidate-speedup cells = oracle_ms / candidate_ms (>1 means candidate faster)."""
    groups = {}
    for c in cells:
        if not c.get("equivalent"):
            continue
        key = (c["consumer"], c["op"])
        groups.setdefault(key, []).append(c)

    def macro(rows):
        vals = [r["oracle_ms"] / r["candidate_ms"] for r in rows if r["candidate_ms"]]
        return sum(vals) / len(vals) if vals else None

    per_group = {}
    for (consumer, op), rows in sorted(groups.items()):
        micro = None
        so = sum(r["oracle_ms"] for r in rows if r["oracle_ms"] is not None)
        sc = sum(r["candidate_ms"] for r in rows if r["candidate_ms"] is not None)
        if sc:
            micro = so / sc
        by_cat = {}
        for r in rows:
            by_cat.setdefault(r["category"], []).append(r)
        by_bucket = {}
        for r in rows:
            by_bucket.setdefault(r["bucket"], []).append(r)
        per_group["%s/%s" % (consumer, op)] = {
            "cells": len(rows),
            "macro_average_by_file": macro(rows),
            "micro_byte_weighted": micro,
            "category_stratified": {k: macro(v) for k, v in sorted(by_cat.items())},
            "size_bucket_stratified": {k: macro(v) for k, v in sorted(by_bucket.items())},
        }
    return per_group


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--tier1", action="store_true")
    ap.add_argument("--ids", default="")
    ap.add_argument("--consumers", default=",".join(CONSUMER_DRIVER))
    ap.add_argument("--providers", default="oracle,candidate")
    ap.add_argument("--reps", type=int, default=None)
    ap.add_argument("--timeout", type=int, default=None,
                    help="override the size-scaled per-op budget (seconds)")
    ap.add_argument("--force", action="store_true")
    args = ap.parse_args()

    with open(MANIFEST, encoding="utf-8") as f:
        manifest = json.load(f)
    with open(REPORT, encoding="utf-8") as f:
        report = json.load(f)
    with open(ELIG, encoding="utf-8") as f:
        elig = json.load(f)
    buckets = {r["id"]: r["bucket"] for r in report["rows"]}
    elig_by_id = {e["id"]: e for e in elig["entries"]}

    consumers = [c for c in args.consumers.split(",") if c]
    providers = [p for p in args.providers.split(",") if p]
    entries = select_ids([e for e in manifest["entries"]], args)

    cells = []
    plan = 0
    for e in entries:
        el = elig_by_id[e["id"]]
        for consumer in consumers:
            ops = el["eligible"].get(consumer, [])
            if not ops:
                continue
            reps, warmup = adaptive(args, e["bytes"])
            budget = op_budget(e["bytes"]) if args.timeout is None else args.timeout
            path = os.path.join(CORPUS_DIR, e["filename"])
            for op in ops:
                plan += 1
                rec = {}
                for mode in providers:
                    fp = cell_path(mode, consumer, e["id"], op)
                    if os.path.exists(fp) and not args.force:
                        with open(fp, encoding="utf-8") as f:
                            rec[mode] = json.load(f)
                        continue
                    data = run_cell(mode, consumer, e["id"], e["category"],
                                    path, [op], reps, warmup, budget)
                    os.makedirs(os.path.dirname(fp), exist_ok=True)
                    data.update({"id": e["id"], "category": e["category"],
                                 "consumer": consumer, "provider": mode,
                                 "bytes": e["bytes"], "ops_requested": [op],
                                 "reps": reps, "budget_s": budget})
                    with open(fp, "w", encoding="utf-8") as f:
                        json.dump(data, f, indent=1, sort_keys=True)
                    rec[mode] = data
                if providers != ["oracle", "candidate"]:
                    continue
                o, c = rec.get("oracle", {}), rec.get("candidate", {})
                oo = (o.get("ops") or {}).get(op, {})
                cc = (c.get("ops") or {}).get(op, {})
                if not oo and o.get("error"):
                    oo = {"ok": False, "error": o["error"]}
                if not cc and c.get("error"):
                    cc = {"ok": False, "error": c["error"]}
                equivalent = bool(oo.get("ok") and cc.get("ok")
                                  and oo.get("fingerprint")
                                  and oo.get("fingerprint") == cc.get("fingerprint"))
                cells.append({
                    "id": e["id"], "category": e["category"],
                    "bucket": buckets.get(e["id"]), "bytes": e["bytes"],
                    "consumer": consumer, "op": op,
                    "oracle_ok": bool(oo.get("ok")), "candidate_ok": bool(cc.get("ok")),
                    "oracle_ms": oo.get("ms"), "candidate_ms": cc.get("ms"),
                    "candidate_speedup": (oo["ms"] / cc["ms"]
                                          if oo.get("ms") and cc.get("ms") else None),
                    "equivalent": equivalent,
                    "oracle_fp": oo.get("fingerprint"), "candidate_fp": cc.get("fingerprint"),
                    "oracle_error": oo.get("error"), "candidate_error": cc.get("error"),
                    "serialize_bytes_identical": _byte_identical(oo, cc),
                    "rss_kb": {"oracle": o.get("rss_kb"), "candidate": c.get("rss_kb")},
                })

    valid = [c for c in cells if c["equivalent"]]
    invalid = [c for c in cells if c["oracle_ok"] and c["candidate_ok"] and not c["equivalent"]]
    notexpr = [c for c in cells if not c["oracle_ok"] and not c["candidate_ok"]]
    errored = [c for c in cells if c["oracle_ok"] != c["candidate_ok"]]
    byte_cmp = [c for c in cells if c.get("serialize_bytes_identical") is not None]
    matrix = {
        "schema": "consumer-matrix/1",
        "phase": "16.12",
        "aggregation_contract": elig.get("aggregation_policy"),
        "plan_cells": plan,
        "cells": cells,
        "counts": {"total": len(cells), "equivalent": len(valid),
                   "invalid_result": len(invalid),
                   "not_expressible": len(notexpr), "asymmetric_error": len(errored),
                   "serialization_byte_checked": len(byte_cmp),
                   "serialization_byte_identical": sum(
                       1 for c in byte_cmp if c["serialize_bytes_identical"])},
        "aggregation": aggregate(cells),
    }
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8") as f:
        json.dump(matrix, f, indent=1, sort_keys=True)
        f.write("\n")
    print("cells=%d equivalent=%d invalid=%d not_expressible=%d asym=%d"
          % (len(cells), len(valid), len(invalid), len(notexpr), len(errored)))
    for k, v in matrix["aggregation"].items():
        print("  %-28s n=%-4d macro=%s micro=%s" % (
            k, v["cells"],
            "%.3f" % v["macro_average_by_file"] if v["macro_average_by_file"] else "-",
            "%.3f" % v["micro_byte_weighted"] if v["micro_byte_weighted"] else "-"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
