#!/usr/bin/env python3
"""Phase 14 — candidate vs oracle header struct-field type-identity diff.

Compares every struct field's canonical (typedef-resolved) C type between the
candidate header tree and the oracle header tree using clang's AST dump.
Reports only REAL type-identity drifts — the class of defect the layout-only
ABI courts cannot see (all pointers are 8 bytes) but real C consumers (lxml,
nokogiri, PHP) hit at compile time.

Usage: header_struct_diff.py [candidate-include-dir] [oracle-include-dir]
Defaults: include /usr/include/libxml2
"""
import json
import os
import subprocess
import sys

CAND = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "include")
ORAC = os.path.abspath(sys.argv[2] if len(sys.argv) > 2 else "/usr/include/libxml2")
SUBDIRS = ("libxml", "libxslt", "libexslt")


def ast_for(header, incdir):
    cmd = ["clang", "-Xclang", "-ast-dump=json", "-fsyntax-only",
           "-I", incdir, header]
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0:
        return None, r.stderr[-2000:]
    return json.loads(r.stdout), None


def walk_records(node, out):
    if node.get("kind") == "RecordDecl" and node.get("completeDefinition"):
        name = node.get("name", "")
        if name.startswith("_"):
            fields = {}
            for c in node.get("inner", []):
                if c.get("kind") == "FieldDecl":
                    t = c.get("type", {})
                    q = t.get("desugaredQualType") or t.get("qualType")
                    fields[c.get("name")] = q
            out[name] = fields
    for c in node.get("inner", []):
        walk_records(c, out)


def collect(headers, incdir):
    records = {}
    errs = []
    for h in headers:
        ast, err = ast_for(h, incdir)
        if ast is None:
            errs.append((os.path.basename(h), err))
            continue
        walk_records(ast, records)
    return records, errs


def main():
    cand_hdrs, orac_hdrs = [], []
    for sub in SUBDIRS:
        cdir, odir = os.path.join(CAND, sub), os.path.join(ORAC, sub)
        if not (os.path.isdir(cdir) and os.path.isdir(odir)):
            continue
        for fn in sorted(os.listdir(cdir)):
            if not fn.endswith(".h"):
                continue
            if os.path.exists(os.path.join(cdir, fn)):
                cand_hdrs.append(os.path.join(cdir, fn))
            if os.path.exists(os.path.join(odir, fn)):
                orac_hdrs.append(os.path.join(odir, fn))
    cand_records, cand_errs = collect(cand_hdrs, CAND)
    orac_records, orac_errs = collect(orac_hdrs, ORAC)
    for h, e in cand_errs + orac_errs:
        print(f"# clang error in {h}:\n{e[:400]}\n")

    def norm(t):
        if t is None:
            return None
        return t.replace("const ", "").replace("restrict ", "")

    found = 0
    for name in sorted(set(cand_records) & set(orac_records)):
        cf, of = cand_records[name], orac_records[name]
        for fname in sorted(set(cf) | set(of)):
            if fname is None:
                continue
            c, o = cf.get(fname), of.get(fname)
            if norm(c) != norm(o):
                found += 1
                print(f"{name}.{fname}: cand={c!r} oracle={o!r}")
    print(f"\n# total field-type differences: {found}")


if __name__ == "__main__":
    main()
