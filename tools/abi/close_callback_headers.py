#!/usr/bin/env python3
"""11.1-L: append callback-family declarations missing from the candidate
headers, verbatim from the oracle 2.15.3 headers (header-surface closure).

Only declarations whose symbols the candidate DSO exports are added (the
headers stay honest-by-construction: every declared function is exported).
"""
import re
import subprocess
import sys

EXCLUDE = {"xmlCtxtPopInput", "xmlCtxtPushInput"}  # 2.15-era, not in ledger surface

def decls(text):
    out = {}
    for m in re.finditer(r'XMLPUBFUN[^;]*;', text, re.S):
        s = m.group(0)
        nm = re.search(r'\b(xml\w+)\s*\(', s)
        if nm:
            out[nm.group(1)] = s.strip()
    return out

def macros(text):
    out = {}
    for m in re.finditer(r'#define\s+(xmlXPath\w+)\b[^\n]*', text):
        out[m.group(1)] = m.group(0)
    return out

def exported():
    r = subprocess.run(["nm", "-D", "--defined-only", "target/debug/liblibxml_rs.so"],
                       capture_output=True, text=True)
    return {l.split()[-1] for l in r.stdout.splitlines() if l.strip()}

def main():
    exp = exported()
    for hdr in sys.argv[1:]:
        cand_path = f"include/libxml/{hdr}"
        cand = open(cand_path).read()
        oracle = open(f"/usr/include/libxml2/libxml/{hdr}").read()
        o = decls(oracle); o.update(macros(oracle))
        c = decls(cand); c.update(macros(cand))
        miss = {k: v for k, v in o.items() if k not in c}
        # filter: for XMLPUBFUN decls, require the symbol to be exported;
        # macros are always allowed (preprocessor-only).
        keep = []
        dropped = []
        for k, v in sorted(miss.items()):
            if k.startswith("xmlXPath") and v.startswith("#define"):
                keep.append(v)
            elif k in EXCLUDE:
                dropped.append(k)
            elif k in exp:
                keep.append(v)
            else:
                dropped.append(k)
        if not keep:
            print(f"{hdr}: nothing to add (dropped {dropped})")
            continue
        block = (
            "\n"
            "/* [11.1-L] begin: callback-family declarations extracted verbatim\n"
            " * from the oracle libxml2 2.15.3 header (only symbols exported by\n"
            " * the candidate DSO are declared). */\n"
            + "\n\n".join(keep)
            + "\n\n/* [11.1-L] end: extracted declarations */\n"
        )
        with open(cand_path, "a") as f:
            f.write(block)
        print(f"{hdr}: appended {len(keep)} declarations (skipped unexported: {dropped})")

if __name__ == "__main__":
    main()
