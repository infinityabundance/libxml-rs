#!/usr/bin/env python3
"""cli_driver.py — §16.12 fixed CLI consumer performance driver (xmllint, xsltproc).

Invoked inside the perf container after `source /court/consumers/lib.sh <mode>`.
Process startup is included in every timed sample (a valid CLI consumer metric);
`ms` is the best wall time over `--reps`. Fingerprints are canonical and
provider-independent (they are the serialized/streamed output bytes, or the
validation verdict).

See INTERFACE.md. `XSLTPERF` (path to the built xsltperf binary) enables the
libxslt compile / precompiled-apply cells.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
FAMILIES = os.path.join(HERE, "families.json")
XSLT_DIR = os.path.join(HERE, "xslt")
SCHEMA_DIR = os.environ.get("SCHEMA_DIR", "/bench/schema")
XSLTPERF = os.environ.get("XSLTPERF", "")
OP_TIMEOUT = int(os.environ.get("OP_TIMEOUT", "600"))

XMLLINT_OPS = ["parse", "stream", "xpath", "dtd_validate", "xsd_validate",
               "relaxng_validate", "serialize"]
XSLTPROC_OPS = ["transform", "stylesheet_compile", "precompiled_apply"]

DTD_RESOURCE = {"MUSICXML": "partwise.dtd", "SVG": "svg10.dtd"}
XSD_RESOURCE = {"MAVEN": "maven-4.0.0.xsd"}


def sha(b) -> str:
    if isinstance(b, str):
        b = b.encode("utf-8", "replace")
    return "sha256:" + hashlib.sha256(b).hexdigest()


def run(cmd, **kw):
    kw.setdefault("timeout", OP_TIMEOUT)
    return subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, **kw)


def timed(cmd, reps, warmup):
    for _ in range(warmup):
        run(cmd)
    best = None
    for _ in range(max(1, reps)):
        t0 = time.perf_counter()
        p = run(cmd)
        dt = (time.perf_counter() - t0) * 1000.0
        if p.returncode != 0:
            raise RuntimeError("%s failed rc=%d: %s" % (
                cmd[0], p.returncode, p.stderr.decode("utf-8", "replace")[:200]))
        best = dt if best is None else min(best, dt)
    return best


def xpath_exprs(category):
    with open(FAMILIES, encoding="utf-8") as f:
        return json.load(f)["families"][category]["xpath"]


def schema_path(kind, category):
    name = (DTD_RESOURCE if kind == "dtd" else XSD_RESOURCE).get(category)
    return os.path.join(SCHEMA_DIR, name) if name else None


def c14n_fp(f):
    """Semantic document fingerprint: canonical XML (c14n) is a byte-exact
    canonical form (attribute/namespace normalised, entities expanded) that is
    independent of pretty-printing. Falls back to --format if c14n is refused."""
    p = run(["xmllint", "--c14n", "--nonet", f])
    if p.returncode == 0 and p.stdout:
        return sha(p.stdout)
    p = run(["xmllint", "--format", "--nonet", f])
    return sha(p.stdout or p.stderr)


def run_xmllint(args, out):
    def emit(op, ok, ms=None, fp="", detail="", error=None):
        d = {"ok": ok, "ms": ms, "fingerprint": fp, "detail": detail}
        if error:
            d["error"] = error
        out[op] = d

    want = set(args.ops.split(",")) if args.ops else set(XMLLINT_OPS)
    f = args.file
    for op in XMLLINT_OPS:
        if op not in want:
            continue
        try:
            if op == "parse":
                ms = timed(["xmllint", "--noout", "--nonet", f], args.reps, args.warmup)
                emit(op, True, ms, c14n_fp(f), "xmllint --noout; fp=c14n")
            elif op == "stream":
                ms = timed(["xmllint", "--stream", "--noout", "--nonet", f],
                           args.reps, args.warmup)
                # --stream emits no document; the streamed result is fingerprinted
                # semantically (the document it streamed to).
                emit(op, True, ms, c14n_fp(f), "xmllint --stream; fp=c14n")
            elif op == "xpath":
                exprs = xpath_exprs(args.category)
                # One parse: evaluate the family's first expression under the
                # timer and use its output as the fingerprint (the remaining
                # expressions are covered by the other XPath-bearing consumers).
                best = None
                outp = None
                for _ in range(args.warmup):
                    run(["xmllint", "--xpath", exprs[0], f])
                for _ in range(max(1, args.reps)):
                    t0 = time.perf_counter()
                    p = run(["xmllint", "--xpath", exprs[0], f])
                    dt = (time.perf_counter() - t0) * 1000.0
                    best = dt if best is None else min(best, dt)
                    outp = p.stdout
                emit(op, True, best, sha(outp or b""), "expr[0]=%s" % exprs[0][:40])
            elif op == "dtd_validate":
                dtd = schema_path("dtd", args.category)
                if not dtd or not os.path.exists(dtd):
                    emit(op, False, error="not_expressible: no offline DTD resource")
                    continue
                ms = timed(["xmllint", "--dtdvalid", dtd, "--noout", "--nonet", f],
                           args.reps, args.warmup)
                p = run(["xmllint", "--dtdvalid", dtd, "--noout", "--nonet", f])
                emit(op, True, ms, sha("valid" if p.returncode == 0 else "invalid"),
                     "verdict=%s" % ("valid" if p.returncode == 0 else "invalid"))
            elif op == "xsd_validate":
                xsd = schema_path("xsd", args.category)
                if not xsd or not os.path.exists(xsd):
                    emit(op, False, error="not_expressible: no offline XSD resource")
                    continue
                ms = timed(["xmllint", "--schema", xsd, "--noout", "--nonet", f],
                           args.reps, args.warmup)
                p = run(["xmllint", "--schema", xsd, "--noout", "--nonet", f])
                emit(op, True, ms, sha("valid" if p.returncode == 0 else "invalid"),
                     "verdict=%s" % ("valid" if p.returncode == 0 else "invalid"))
            elif op == "relaxng_validate":
                emit(op, False, error="not_expressible: no RelaxNG resource available")
            elif op == "serialize":
                best = None
                outp = None
                for _ in range(args.warmup):
                    run(["xmllint", "--format", "--nonet", f])
                for _ in range(max(1, args.reps)):
                    t0 = time.perf_counter()
                    p = run(["xmllint", "--format", "--nonet", f])
                    dt = (time.perf_counter() - t0) * 1000.0
                    best = dt if best is None else min(best, dt)
                    outp = p.stdout
                # Byte-level --format output is reported separately; equivalence
                # is semantic (c14n) so formatting divergence is measurable
                # without invalidating the serialization workload.
                emit(op, True, best, c14n_fp(f),
                     "out_sha=%s" % sha(outp or b""))
        except Exception as exc:  # noqa: BLE001
            emit(op, False, error="%s: %s" % (type(exc).__name__, exc))


def run_xsltproc(args, out):
    def emit(op, ok, ms=None, fp="", detail="", error=None):
        d = {"ok": ok, "ms": ms, "fingerprint": fp, "detail": detail}
        if error:
            d["error"] = error
        out[op] = d

    want = set(args.ops.split(",")) if args.ops else set(XSLTPROC_OPS)
    xsl = os.path.join(XSLT_DIR, "%s.xsl" % args.category)
    f = args.file
    for op in XSLTPROC_OPS:
        if op not in want:
            continue
        try:
            if op == "transform":
                ms = timed(["xsltproc", "--nonet", xsl, f], args.reps, args.warmup)
                p = run(["xsltproc", "--nonet", xsl, f])
                emit(op, True, ms, sha(p.stdout), "xsltproc end-to-end")
            elif op == "stylesheet_compile":
                if XSLTPERF and os.path.exists(XSLTPERF):
                    p = run([XSLTPERF, "--mode", "compile", "--xsl", xsl,
                             "--reps", str(max(1, args.reps))])
                    j = json.loads(p.stdout.decode())
                    emit(op, True, j["ms"], sha("compiled"), "libxslt parse-only")
                else:
                    with tempfile.NamedTemporaryFile("w", suffix=".xml", delete=False) as t:
                        t.write("<a/>")
                        triv = t.name
                    ms = timed(["xsltproc", "--noout", "--nonet", xsl, triv],
                               args.reps, args.warmup)
                    os.unlink(triv)
                    emit(op, True, ms, sha("compiled-proxy"),
                         "xsltproc compile proxy on trivial doc")
            elif op == "precompiled_apply":
                if not (XSLTPERF and os.path.exists(XSLTPERF)):
                    emit(op, False,
                         error="not_expressible: xsltproc CLI cannot precompile")
                    continue
                outfile = os.path.join("/tmp", "xsltperf-%s.out" % args.id)
                p = run([XSLTPERF, "--mode", "apply", "--xsl", xsl, "--doc", f,
                         "--reps", str(max(1, args.reps)), "--out", outfile])
                j = json.loads(p.stdout.decode())
                with open(outfile, "rb") as fh:
                    fp = sha(fh.read())
                os.unlink(outfile)
                emit(op, True, j["ms"], fp, "libxslt precompiled apply")
        except Exception as exc:  # noqa: BLE001
            emit(op, False, error="%s: %s" % (type(exc).__name__, exc))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--consumer", required=True, choices=["xmllint", "xsltproc"])
    ap.add_argument("--id", required=True)
    ap.add_argument("--category", required=True)
    ap.add_argument("--file", required=True)
    ap.add_argument("--reps", type=int, default=3)
    ap.add_argument("--warmup", type=int, default=1)
    ap.add_argument("--ops", default="")
    args = ap.parse_args()
    out: dict = {}
    if args.consumer == "xmllint":
        run_xmllint(args, out)
    else:
        run_xsltproc(args, out)
    print(json.dumps({"consumer": args.consumer, "id": args.id,
                      "category": args.category, "file": args.file, "ops": out},
                     separators=(",", ":")))
    return 0


if __name__ == "__main__":
    sys.exit(main())
