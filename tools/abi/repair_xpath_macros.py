#!/usr/bin/env python3
"""Repair the [11.1-L] appended macro definitions in xpathInternals.h:
multi-line #define bodies were truncated to the first line by the initial
extraction. Re-extracts each xmlXPath* macro verbatim (continuations included)
from the oracle 2.15.3 header and replaces the truncated copies."""
import re

CAND = "include/libxml/xpathInternals.h"
ORACLE = "/usr/include/libxml2/libxml/xpathInternals.h"

cand = open(CAND).read()
oracle = open(ORACLE).read()

# Extract full macro definitions (with continuations) from the oracle.
def oracle_macros(text):
    out = {}
    for m in re.finditer(r'#define\s+(xmlXPath\w+)\b[^\n]*(?:\n[ \t]+[^\n]*)*', text):
        out[m.group(1)] = m.group(0)
    return out

oms = oracle_macros(oracle)

def block_macros(text):
    # macros inside the [11.1-L] block
    start = text.find("/* [11.1-L] begin")
    end = text.find("/* [11.1-L] end", start)
    blk = text[start:end]
    out = {}
    for m in re.finditer(r'#define\s+(xmlXPath\w+)\b[^\n]*(?:\n[ \t]+[^\n]*)*', blk):
        out[m.group(1)] = m.group(0)
    return out

bms = block_macros(cand)
fixed = 0
for name, full in bms.items():
    oracle_full = oms.get(name)
    if oracle_full is None:
        continue
    if full != oracle_full:
        # replace the truncated macro with the oracle-verbatim one
        cand = cand.replace(full, oracle_full)
        fixed += 1
        print(f"fixed {name}")

open(CAND, "w").write(cand)
print(f"{fixed} macros repaired")
