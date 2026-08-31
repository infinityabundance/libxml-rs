#!/usr/bin/env python3
"""Locate the unaccounted unsafe blocks precisely (module, fn, line)."""
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from evidence.custodian_commentary_drift import (  # noqa: E402
    fn_doc, fn_regions, module_scope_id,
)

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

TARGETS = ["src/xml/c14n/mod.rs", "src/xml/entities/mod.rs", "src/xml/globals/mod.rs",
           "src/xml/html/mod.rs", "src/xml/io/mod.rs", "src/xml/tree/mod.rs",
           "src/xml/writer/mod.rs", "src/xslt/security/mod.rs",
           "src/xslt/serialization/mod.rs"]

for rel in TARGETS:
    path = os.path.join(ROOT, rel)
    text = open(path, encoding="utf-8", errors="replace").read()
    fns = fn_regions(text)
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
        line = text[:pos].count("\n") + 1
        if owner:
            name, fs = owner
            doc = fn_doc(text, fs)
            if re.search(r"#\s*Safety\b", doc, re.I):
                continue
            print(f"{rel}:{line}  fn {name} (no # Safety in doc)")
        else:
            print(f"{rel}:{line}  <no enclosing fn>")
