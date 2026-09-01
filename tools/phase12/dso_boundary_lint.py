#!/usr/bin/env python3
"""DSO-BOUNDARY-LINT (Phase 12, R-000177) — machine-enforced DSO boundary lint.

Architectural invariant (the reviewer's Phase-12 target for R-000177):

    libxslt implementation
            |  may call:  crate::xslt::*           (same-DSO internals)
            |             crate::abi::exports_*    (SANCTIONED libxml2 public
            |                                       ABI gateway — the surface
            |                                       a thin-facade DSO could
            |                                       resolve across the boundary)
            |             crate::abi::{structs, types, constants, callbacks}
            |                                       (ABI type surface, layout only)
            |             the five exported allocator DATA variables
            |                                       (xmlMalloc/xmlFree/...)
            |  may NOT call:
            |             crate::xml::*            (libxml2 implementation
            |                                       internals: parser/tree/xpath/
            |                                       io/string/hash/...)
            |             crate::abi::allocator::*Impl & helpers
            |                                       (allocator implementation
            |                                       internals)
            |             crate::abi::data_globals (XML-layer private globals)
            v
    libxml2.so.16

The current whole-archive facade construction (11.1-Z.1) intentionally
contains a private copy of the entire core inside each facade, so these
boundary violations do not cross a real DSO boundary at load time — they are
resolved internally — but they are exactly what partitions process state
(DSO-STATE-COHERENCE court, R-000177 OPEN): a future state-coherent
thin-facade architecture requires the xslt layer to route every libxml2
interaction through the sanctioned gateway.

This lint is the machine enforcement for that target. It scans the libxslt
implementation scope (src/xslt/** and the src/abi/exports_xslt* ABI layer)
and classifies EVERY crate:: path reference. It is FAIL-CLOSED: any
reference that cannot be classified is a finding (silent omissions = 0). The
verdict does NOT require zero violations — R-000177 is the deliberately
open architectural target — but it requires:

  - unclassified references == 0
  - the violation census is complete and recorded (the R-000177 surface)
  - every SANCTIONED_GATEWAY reference resolves to a module under
    src/abi/exports_* that exists

Evidence: courts/receipts/phase-12/dso-boundary-lint-<ts>.json

Usage:
    python3 tools/phase12/dso_boundary_lint.py
"""
import datetime
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-12")

# The libxslt implementation scope: the XSLT engine + the libxslt ABI layer.
# These compile into libxslt.so.1; their crate:: references define the DSO
# boundary the lint enforces.
SCOPE_DIRS = ["src/xslt"]
SCOPE_FILE_PREFIXES = ("exports_xslt", "exports_exslt")

# Sanctioned libxml2 public-ABI gateway modules (cross-DSO-resolvable).
GATEWAY_MODULES = ("crate::abi::exports_",)

# ABI type surface (layout-only; no implementation state).
TYPE_SURFACE = ("crate::abi::structs", "crate::abi::types",
                "crate::abi::constants", "crate::abi::callbacks")

# Pure helper module (string helpers, no libxml state).
HELPERS = ("crate::abi::versioning",)

# The exported allocator DATA variables (public data ABI, R-000176).
ALLOCATOR_VARS = {
    "xmlMalloc", "xmlMallocAtomic", "xmlRealloc", "xmlFree", "xmlMemStrdup",
    "__xmlMalloc", "__xmlMallocAtomic", "__xmlRealloc", "__xmlFree",
    "__xmlMemStrdup",
}

# XSLT-owned globals hosted in the shared data_globals module (R-000174
# xsltGenericDebug family) — same-DSO data.
XSLT_DATA_PREFIX = "crate::abi::data_globals::xslt"

REF_RE = re.compile(r"crate::[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*")
USE_REF_RE = re.compile(r"use\s+(crate::[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*)")
# `use crate::a::b::{c, d}` — capture the group-inner idents too.
USE_GROUP_RE = re.compile(r"use\s+(crate::[A-Za-z_][A-Za-z0-9_]*)(?:::[A-Za-z_][A-Za-z0-9_]*)*\s*::\s*\{([^}]*)\}")


def is_in_scope(path):
    rel = os.path.relpath(path, ROOT)
    for d in SCOPE_DIRS:
        if rel.startswith(d + os.sep):
            return True
    base = os.path.basename(rel)
    if rel.startswith("src" + os.sep + "abi" + os.sep) and base.startswith(SCOPE_FILE_PREFIXES):
        return True
    return False


def classify(full_path, leaf=None):
    """Classify a crate:: reference path."""
    # XSLT's own internals — same DSO, always allowed.
    if full_path == "crate::xslt" or full_path.startswith("crate::xslt::"):
        return "OWN_INTERNALS"
    # The EXSLT engine is the third DSO of the same combined core; its
    # internals are same-package (the exslt facade needs xslt registration
    # and vice versa).
    if full_path == "crate::exslt" or full_path.startswith("crate::exslt::"):
        return "OWN_INTERNALS"
    # XSLT-owned data globals hosted in the shared data_globals module
    # (R-000174 xsltGenericDebug family, xslDebugStatus).
    if full_path.startswith("crate::abi::data_globals::"):
        if leaf in ("xsltGenericDebug", "xsltGenericDebugContext",
                    "xsltGenericError", "xsltGenericErrorContext",
                    "xslDebugStatus"):
            return "OWN_INTERNALS"
    # Sanctioned libxml2 public ABI gateway.
    if full_path.startswith(GATEWAY_MODULES):
        return "SANCTIONED_GATEWAY"
    # ABI type surface.
    if full_path in TYPE_SURFACE or full_path.startswith(tuple(m + "::" for m in TYPE_SURFACE)):
        return "SANCTIONED_TYPE_SURFACE"
    # Pure helpers.
    if full_path in HELPERS or full_path.startswith(tuple(m + "::" for m in HELPERS)):
        return "SANCTIONED_HELPER"
    # ABI struct types are referenced by their underscore-prefixed names
    # (crate::abi::_xsltStylesheet, _xsltDecimalFormat, ...).
    if leaf and leaf.startswith("_") and full_path.startswith("crate::abi::"):
        return "SANCTIONED_TYPE_SURFACE"
    # Allocator: the exported DATA variables are the public data ABI; the
    # *Impl bodies / helpers are implementation internals.
    if full_path.startswith("crate::abi::allocator"):
        if leaf in ALLOCATOR_VARS:
            return "SANCTIONED_GATEWAY"
        return "BOUNDARY_VIOLATION"
    # Allocator implementation internals re-exported at the abi root.
    if leaf in ("xmlFreeImpl", "xmlMallocImpl", "xmlMallocAtomicImpl",
                "xmlReallocImpl", "xmlMemStrdupImpl", "xmlMallocZero"):
        return "BOUNDARY_VIOLATION"
    # libxml2 implementation internals (the whole crate::xml::* plane).
    if full_path.startswith("crate::xml::"):
        return "BOUNDARY_VIOLATION"
    # XML-layer private globals.
    if full_path.startswith("crate::abi::data_globals"):
        return "BOUNDARY_VIOLATION"
    # crate::abi root re-exports of #[no_mangle] exported functions are the
    # sanctioned gateway surface (xmlReadMemory, xmlBufferCreate, ...).
    if full_path.startswith("crate::abi::"):
        return "SANCTIONED_GATEWAY"
    return "UNCLASSIFIED"


def scan():
    refs = {}  # full_path -> {"class": ..., "files": set, "count": n}
    for root, _dirs, files in os.walk(os.path.join(ROOT, "src")):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            path = os.path.join(root, fn)
            if not is_in_scope(path):
                continue
            text = open(path, encoding="utf-8", errors="replace").read()
            # plain `crate::...` path expressions
            for m in REF_RE.finditer(text):
                full = m.group(0)
                leaf = full.rsplit("::", 1)[-1]
                entry = refs.setdefault(full, {"class": None, "files": set(), "count": 0})
                entry["files"].add(os.path.relpath(path, ROOT))
                entry["count"] += 1
                # remember the leaf of the FIRST occurrence for classification
                if entry["class"] is None:
                    entry["_leaf"] = leaf
            # `use crate::...;` statements and group imports
            for m in USE_REF_RE.finditer(text):
                full = m.group(1)
                leaf = full.rsplit("::", 1)[-1]
                entry = refs.setdefault(full, {"class": None, "files": set(), "count": 0})
                entry["files"].add(os.path.relpath(path, ROOT))
                entry["count"] += 1
                if entry["class"] is None:
                    entry["_leaf"] = leaf
            for m in USE_GROUP_RE.finditer(text):
                base = m.group(1)
                for inner in m.group(2).split(","):
                    inner = inner.strip().split(" as ")[0].strip()
                    if not inner:
                        continue
                    full = f"{base}::{inner}"
                    leaf = inner
                    entry = refs.setdefault(full, {"class": None, "files": set(), "count": 0})
                    entry["files"].add(os.path.relpath(path, ROOT))
                    entry["count"] += 1
                    if entry["class"] is None:
                        entry["_leaf"] = leaf
    return refs


def main():
    os.makedirs(RECEIPTS, exist_ok=True)
    refs = scan()

    classified = {}
    violations = []
    unclassified = []
    gateway_missing = []
    for full, info in sorted(refs.items()):
        cls = classify(full, info.get("_leaf"))
        info["class"] = cls
        files = sorted(info["files"])
        entry = {"reference": full, "class": cls, "count": info["count"],
                 "files": files}
        classified[full] = entry
        if cls == "BOUNDARY_VIOLATION":
            violations.append(entry)
        elif cls == "UNCLASSIFIED":
            unclassified.append(entry)
        elif cls == "SANCTIONED_GATEWAY":
            # gateway MODULE references (crate::abi::exports_xxx::sym) must
            # resolve to a real module file; bare crate::abi::sym re-exports
            # are resolved by the compiler itself.
            parts = full.split("::")
            if len(parts) >= 4 and parts[2].startswith("exports_"):
                if not os.path.exists(os.path.join(ROOT, "src", "abi", parts[2] + ".rs")):
                    gateway_missing.append(entry)

    counts = {}
    for e in classified.values():
        counts[e["class"]] = counts.get(e["class"], 0) + 1

    fail_closed = len(unclassified) == 0 and len(gateway_missing) == 0
    verdict = "PASS" if fail_closed else "FAIL"

    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    receipt = {
        "court": "DSO-BOUNDARY-LINT",
        "phase": "12",
        "schema": "dso-boundary-lint-1",
        "timestamp": ts,
        "residual": "R-000177 (OPEN architectural target: state-coherent "
                    "three-DSO architecture requires the xslt layer to route "
                    "every libxml2 interaction through the sanctioned "
                    "gateway; the whole-archive facades currently partition "
                    "state — pinned by the DSO-STATE-COHERENCE court)",
        "invariant": "libxslt implementation may call crate::xslt::* (own), "
                     "crate::abi::exports_* (sanctioned gateway), the ABI "
                     "type surface, and the exported allocator data "
                     "variables; may NOT call crate::xml::* internals, "
                     "allocator *Impl internals, or XML-layer private "
                     "globals",
        "verdict_gates": "fail-closed classification only (unclassified == 0 "
                         "and every sanctioned gateway module resolves); "
                         "violation COUNT is the R-000177 surface, not a "
                         "verdict gate — R-000177 is deliberately OPEN",
        "scope": {"dirs": SCOPE_DIRS,
                  "abi_file_prefixes": list(SCOPE_FILE_PREFIXES)},
        "counts": counts,
        "violation_surface": violations,
        "unclassified": unclassified,
        "gateway_missing": gateway_missing,
        "verdict": verdict,
    }
    rp = os.path.join(RECEIPTS, f"dso-boundary-lint-{ts}.json")
    with open(rp, "w") as f:
        json.dump(receipt, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"receipt -> {rp}")
    print(f"counts: {counts}")
    print(f"unclassified={len(unclassified)} gateway_missing={len(gateway_missing)} "
          f"verdict={verdict}")
    return 0 if fail_closed else 1


if __name__ == "__main__":
    sys.exit(main())
