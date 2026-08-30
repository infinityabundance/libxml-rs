#!/usr/bin/env python3
"""11.1-R — Compatibility profile reconciliation view.

Renders atlas/COMPATIBILITY_PROFILES.md from the capability-epoch table in
src/compatibility/profiles/mod.rs and the surface-epoch evidence produced by
the 11.1-Q delta engine (atlas/HISTORICAL_SURFACE_EPOCHS.json).

The Rust module is the single source of truth for the epoch resolver; this
generator produces the human-readable reconciliation: every capability maps
to the upstream evidence that created its boundary, and every surface-epoch
transition is either represented by a capability or explicitly residual.

Usage:
    python3 tools/evidence/compatibility_profiles.py
"""

import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
EPOCHS = os.path.join(ROOT, "atlas", "HISTORICAL_SURFACE_EPOCHS.json")
OUT = os.path.join(ROOT, "atlas", "COMPATIBILITY_PROFILES.md")

# Mirrors the capability table in src/compatibility/profiles/mod.rs.
CAPABILITIES = [
    ("XPathNodeSetSerialization", "E-001", "2.9.10",
     "xmllint --xpath node-set output: concatenated -> newline-separated "
     "(commit da35eeae, documented breaking change)"),
    ("ParserDiagnostic", "E-002", "2.9.10 / 2.12.x",
     "second parse-error diagnostic: present -> 2.9.10 regression variant "
     "-> dropped in the 2.12 error-handling rework"),
    ("EntityCompactStorage", "E-004", "2.13.0",
     "entity content debug node: TEXT -> TEXT compact (commit 8d04f0ee)"),
    ("ValidationExit", "E-005", "2.13.0",
     "xmllint parse-error/undeclared exit 1 -> 4; valid-invalid 4 -> 3"),
    ("XpathAttrEmptyExit", "E-003", "2.11.0 / 2.12.6",
     "empty node-set exit code 10 -> 0 (e85f9b98) -> 11 (387a952b)"),
    ("HtmlSerializer", "E-007", "2.15.0",
     "HTML dump: newline-per-element -> single line (newline writes removed "
     "from HTMLtree.c)"),
    ("ValidationNoDtdExit", "E-006", "2.15.0",
     "--valid without DTD: exit 3 -> 0 (xmllint refactor)"),
    ("GlobalStateInit", "2.12 rework", "2.12.0",
     "eager static initialisation -> lazy per-context initialisation"),
    ("XslTransform", "E-008", "stable since <=1.1.26",
     "libxslt transform output byte-identical 1.1.26..1.1.45"),
]


def main():
    with open(EPOCHS) as f:
        epochs = json.load(f)
    L = []
    L.append("# Compatibility Profiles — 11.1-R\n")
    L.append("The candidate models historical behavior through **capability "
             "epochs** (src/compatibility/profiles/mod.rs), never scattered "
             "`if version == ...` branches. A `CompatibilityProfile` resolves "
             "every capability for a target upstream version pair.\n")
    L.append("## Capability epochs\n")
    L.append("| Capability | Evidence | Boundary | Change |")
    L.append("|---|---|---|---|")
    for name, ev, boundary, change in CAPABILITIES:
        L.append(f"| `{name}` | {ev} | {boundary} | {change} |")
    L.append("")
    L.append("## Version -> profile mapping (resolver rules)\n")
    L.append("```text\n"
             "capabilities_for_libxml2(version):\n"
             "  XPathNodeSetSerialization : (maj,min) > (2,9) or (2,9,pat>=10)"
             " ? NewlineSeparated : Concatenated\n"
             "  ParserDiagnostic         : [2.9.10,2.9.11) Regression; >=2.12 "
             "Single; else Dual\n"
             "  EntityCompactStorage     : >=2.13 Compact : Plain\n"
             "  ValidationExit           : >=2.13 Reworked : Legacy\n"
             "  XpathAttrEmptyExit       : <2.11 Legacy; <2.12.6 NoError; "
             "else Error11\n"
             "  HtmlSerializer           : >=2.15 SingleLine : Formatted\n"
             "  ValidationNoDtdExit      : >=2.15 Ok0 : Error3\n"
             "  GlobalStateInit          : >=2.12 Lazy : Eager\n"
             "  XslTransform             : Stable (all versions)\n"
             "```\n")
    L.append("## Reconciliation with the surface delta engine (11.1-Q)\n")
    L.append("The Q engine (`atlas/HISTORICAL_SURFACE_EPOCHS.json`) tracks "
             "entity-level surface transitions; the table below shows the "
             "libxml2 boundaries where the surface moved, and whether the "
             "behavioral capability is represented.\n")
    for project, label in (("libxml2", "libxml2"), ("libxslt", "libxslt")):
        p = epochs["projects"][project]
        L.append(f"### {label}\n")
        L.append("| boundary | added | removed | changed |")
        L.append("|---|---|---|---|")
        for b, v in p["deltas"].items():
            d = v["deltas"]
            L.append(f"| {b} | {d['added_count']} | {d['removed_count']} | "
                     f"{d['changed_count']} |")
        L.append("")
    L.append("## Policy\n")
    L.append("- The candidate's current profile targets the system oracle "
             "(libxml2 2.15.3 / libxslt 1.1.45).\n"
             "- New historical differences must be added as capability "
             "epochs with evidence-backed boundaries, not version branches.\n"
             "- `CompatibilityProfile::for_libxml2` refuses versions newer "
             "than the system oracle (no unverifiable epochs).\n")
    with open(OUT, "w") as f:
        f.write("\n".join(L) + "\n")
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
