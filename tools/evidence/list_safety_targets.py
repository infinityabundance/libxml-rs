#!/usr/bin/env python3
"""List the (module, fn) targets that need a `# Safety` section in their doc
comment (or an fn-attached SAFETY-SCOPE marker) to reach unaccounted=0 in the
CUSTODIAN-COMMENTARY-DRIFT proof-scope census (11.1-Z.3).

Output: a JSON mapping module -> [fn names] for modules with unaccounted
unsafe blocks, plus the list of undocumented unsafe fns.
"""
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from custodian_commentary_drift import (  # noqa: E402
    FN_RE, SAFETY_SCOPE_RE, fn_attached_scope, fn_doc, fn_regions, module_scope_id,
)

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, "src")


def needs_safety_section(text, fn_start):
    doc = fn_doc(text, fn_start)
    if re.search(r"#\s*Safety\b", doc, re.I):
        return False
    return True


def main():
    targets = {}
    undoc_fns = []
    for root, _d, files in os.walk(SRC):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            rel = os.path.relpath(os.path.join(root, fn), ROOT)
            text = open(os.path.join(root, fn), encoding="utf-8", errors="replace").read()
            fns = fn_regions(text)
            mscope = module_scope_id(text)
            unacct = set()
            for m in re.finditer(r"unsafe\s*\{", text):
                pos = m.start()
                window = text[max(0, pos - 400):pos + 400]
                if re.search(r"//\s*SAFETY\b|#\s*Safety\b", window, re.I):
                    continue
                owner = None
                for (name, fs, si, ei) in fns:
                    if si < pos < ei:
                        owner = (name, fs)
                        break
                if owner:
                    name, fs = owner
                    if re.search(r"#\s*Safety\b", fn_doc(text, fs), re.I):
                        continue
                    if fn_attached_scope(text, fs):
                        continue
                    unacct.add(name)
                elif not mscope:
                    unacct.add("<module-level>")
            if unacct:
                targets[rel] = sorted(unacct)
            # undocumented unsafe fns
            for m in re.finditer(
                    r"(?:pub(?:\([^)]*\))?(?:\s+const)?\s+)?unsafe(?:"
                    r"\s+extern\s+\"C\"\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)",
                    text):
                if not fn_doc(text, m.start()) and not mscope:
                    undoc_fns.append((rel, m.group(1)))
    print(json.dumps({"fn_targets": targets, "undoc_unsafe_fns": undoc_fns},
                     indent=1))


if __name__ == "__main__":
    main()
