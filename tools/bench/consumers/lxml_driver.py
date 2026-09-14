#!/usr/bin/env python3
"""lxml_driver.py — §16.12.3 fixed python3-lxml performance driver.

Invoked inside the perf container with a provider already selected by
`source /court/consumers/lib.sh <oracle|candidate>`. Emits one single-line JSON
object (see INTERFACE.md).

Ops: dom_parse, iterparse, xpath_adhoc, xpath_compiled, xpath_compile, tostring,
xslt, dtd_validate, xsd_validate.

`ms` is the best of `--reps` monotonic engine timings after `--warmup` warmups.
Fingerprints are canonical and provider-independent.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import time

from lxml import etree

HERE = os.path.dirname(os.path.abspath(__file__))
FAMILIES = os.path.join(HERE, "families.json")
XSLT_DIR = os.path.join(HERE, "xslt")
SCHEMA_DIR = os.environ.get("SCHEMA_DIR", "/bench/schema")

OPS = ["dom_parse", "iterparse", "xpath_adhoc", "xpath_compiled",
       "xpath_compile", "tostring", "xslt", "dtd_validate", "xsd_validate"]

# category -> offline DTD/XSD resource file (only where a single self-contained
# resource exists; otherwise the op reports not_expressible).
DTD_RESOURCE = {
    "MUSICXML": "partwise.dtd",
    "SVG": "svg10.dtd",
}
XSD_RESOURCE = {
    "MAVEN": "maven-4.0.0.xsd",
}


def sha(s: str) -> str:
    return "sha256:" + hashlib.sha256(s.encode("utf-8", "replace")).hexdigest()


def norm(s: str) -> str:
    return " ".join(s.split())


def canon(node, depth=0, out=None):
    if out is None:
        out = []
    tag = node.tag
    if not isinstance(tag, str):  # comment / processing instruction
        out.append("N %s %d" % (getattr(tag, "__name__", "node"), depth))
    else:
        out.append("E %s %d" % (tag, depth))
        for k in sorted(node.attrib):
            out.append("A %s=%s" % (k, node.attrib[k]))
        if node.text and node.text.strip():
            out.append("T %s" % norm(node.text))
    for c in node:
        canon(c, depth + 1, out)
        if c.tail and c.tail.strip():
            out.append("X %s" % norm(c.tail))
    return out


def canon_doc(root) -> str:
    return "\n".join(canon(root))


def result_fp(tree, results) -> str:
    parts = []
    for r in results:
        if isinstance(r, bool):
            parts.append("B:%d" % (1 if r else 0))
        elif isinstance(r, float):
            parts.append("N:%.17g" % r)
        elif isinstance(r, str):
            parts.append("S:%s" % r)
        elif isinstance(r, list):
            parts.append("L:%d" % len(r))
            for item in r:
                if isinstance(item, etree._Element):
                    parts.append("P:%s" % tree.getpath(item))
                else:
                    parts.append("V:%s" % item)
        else:
            parts.append("O:%s" % r)
    return "\n".join(parts)


def best_time(fn, reps, warmup):
    for _ in range(warmup):
        fn()
    b = None
    for _ in range(max(1, reps)):
        t0 = time.perf_counter()
        fn()
        dt = (time.perf_counter() - t0) * 1000.0
        b = dt if b is None else min(b, dt)
    return b


def op_set(args, ops):
    want = set(ops.split(",")) if ops else set(OPS)
    return [o for o in OPS if o in want]


def run(args) -> dict:
    path = args.file
    xsl = os.path.join(XSLT_DIR, "%s.xsl" % args.category)
    with open(FAMILIES, encoding="utf-8") as f:
        xpath_exprs = json.load(f)["families"][args.category]["xpath"]
    out = {}

    def emit(op, ok, ms=None, fp="", detail="", error=None):
        d = {"ok": ok, "ms": ms, "fingerprint": fp, "detail": detail}
        if error:
            d["error"] = error
        out[op] = d

    for op in op_set(args, args.ops):
        try:
            if op == "dom_parse":
                fp = {"v": None}

                def go():
                    t = etree.parse(path)
                    fp["v"] = sha(canon_doc(t.getroot()))
                ms = best_time(go, args.reps, args.warmup)
                emit(op, True, ms, fp["v"], "lxml.etree.parse")
            elif op == "iterparse":
                fp = {"v": None}

                def go():
                    parts = []
                    for ev, el in etree.iterparse(path, events=("start", "end")):
                        parts.append("%s %s" % (ev, el.tag))
                        if ev == "end":
                            el.clear()
                    fp["v"] = sha("\n".join(parts))
                ms = best_time(go, args.reps, args.warmup)
                emit(op, True, ms, fp["v"], "iterparse events")
            elif op in ("xpath_adhoc", "xpath_compiled", "xpath_compile"):
                tree = etree.parse(path)
                if op == "xpath_adhoc":
                    fp = {"v": None}

                    def go():
                        rs = [tree.xpath(e) for e in xpath_exprs]
                        fp["v"] = sha(result_fp(tree, rs))
                    ms = best_time(go, args.reps, args.warmup)
                    emit(op, True, ms, fp["v"], "exprs=%d" % len(xpath_exprs))
                elif op == "xpath_compiled":
                    compiled = [etree.XPath(e) for e in xpath_exprs]
                    fp = {"v": None}

                    def go():
                        rs = [c(tree) for c in compiled]
                        fp["v"] = sha(result_fp(tree, rs))
                    ms = best_time(go, args.reps, args.warmup)
                    emit(op, True, ms, fp["v"], "compiled exprs=%d" % len(compiled))
                else:
                    fp = {"v": None}

                    def go():
                        cs = [etree.XPath(e) for e in xpath_exprs]
                        fp["v"] = sha("compile:%d" % len(cs))
                    ms = best_time(go, args.reps, args.warmup)
                    emit(op, True, ms, fp["v"], "compile exprs=%d" % len(xpath_exprs))
            elif op == "tostring":
                tree = etree.parse(path)
                state = {"fp": None, "unstable": False}

                def go():
                    s = etree.tostring(tree, encoding="unicode")
                    if state["fp"] is None:
                        state["fp"] = sha(s)
                    elif s != "" and sha(s) != state["fp"]:
                        state["unstable"] = True
                    elif s == "":
                        state["unstable"] = True
                ms = best_time(go, args.reps, args.warmup)
                if state["unstable"]:
                    emit(op, False,
                         error="serialization_instability: repeated etree.tostring "
                               "returned differing/empty output")
                else:
                    emit(op, True, ms, state["fp"], "etree.tostring")
            elif op == "xslt":
                xsl_doc = etree.parse(xsl)
                tree = etree.parse(path)
                fp = {"v": None}

                def go():
                    res = etree.XSLT(xsl_doc)(tree)
                    fp["v"] = sha(str(res))
                ms = best_time(go, args.reps, args.warmup)
                emit(op, True, ms, fp["v"], "etree.XSLT")
            elif op == "dtd_validate":
                name = DTD_RESOURCE.get(args.category)
                dtd = os.path.join(SCHEMA_DIR, name) if name else None
                if not dtd or not os.path.exists(dtd):
                    emit(op, False, error="not_expressible: no offline DTD resource")
                    continue
                schema = etree.DTD(dtd)
                tree = etree.parse(path)
                fp = {"v": None}

                def go():
                    fp["v"] = "valid" if schema.validate(tree) else "invalid"
                ms = best_time(go, args.reps, args.warmup)
                emit(op, True, ms, sha(fp["v"]), "verdict=%s" % fp["v"])
            elif op == "xsd_validate":
                name = XSD_RESOURCE.get(args.category)
                xsd = os.path.join(SCHEMA_DIR, name) if name else None
                if not xsd or not os.path.exists(xsd):
                    emit(op, False, error="not_expressible: no offline XSD resource")
                    continue
                schema = etree.XMLSchema(etree.parse(xsd))
                tree = etree.parse(path)
                fp = {"v": None}

                def go():
                    fp["v"] = "valid" if schema.validate(tree) else "invalid"
                ms = best_time(go, args.reps, args.warmup)
                emit(op, True, ms, sha(fp["v"]), "verdict=%s" % fp["v"])
        except Exception as exc:  # noqa: BLE001
            emit(op, False, error="%s: %s" % (type(exc).__name__, exc))

    return {"consumer": "python3-lxml", "id": args.id, "category": args.category,
            "file": path, "ops": out}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--id", required=True)
    ap.add_argument("--category", required=True)
    ap.add_argument("--file", required=True)
    ap.add_argument("--reps", type=int, default=5)
    ap.add_argument("--warmup", type=int, default=2)
    ap.add_argument("--ops", default="")
    args = ap.parse_args()
    print(json.dumps(run(args), separators=(",", ":")))
    return 0


if __name__ == "__main__":
    sys.exit(main())
