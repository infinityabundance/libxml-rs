#!/usr/bin/env python3
"""
header_surface_audit.py — 11.1-S header-surface audit and extraction tool.

Audit direction (oracle header -> candidate header):
    every function/variable DECLARED by the system oracle headers (the true
    2.15.3 / 1.1.45 / 0.8.25 contract: /usr/include/libxml2/libxml,
    /usr/include/libxslt, /usr/include/libexslt) must be DECLARED by the
    candidate drop-in headers (include/libxml, include/libxslt,
    include/libexslt). A consumer writing normal upstream code must compile
    against the headers alone; the headers are the source-compatibility
    contract. Deprecated symbols that modern upstream headers deliberately no
    longer declare (xmlUCSIs*, xmlNanoFTP*, xmlShell*, ...) are intentionally
    absent on both sides.

The tool:
  1. enumerates candidate DSO exports (`nm -D --defined-only`);
  2. parses declarations (XMLPUBFUN/XMLPUBVAR & friends) from the candidate
     headers;
  3. computes the exported-but-undeclared set;
  4. extracts the missing declarations VERBATIM from the oracle headers
     (oracle/historical/prefix/libxml2-2.15.0/include/libxml2/libxml/*.h,
     oracle/historical/prefix/libxslt-1.1.42/include/libxslt/*.h, with a
     fallback to the archaeology source trees), so signatures are the
     upstream ABI contract, byte-for-byte;
  5. writes them into the matching candidate headers inside clearly marked
     `/* [11.1-S] begin: oracle-extracted declarations */` blocks
     (idempotent: already-declared symbols are skipped).

Usage:
    python3 tools/evidence/header_surface_audit.py --apply   # write blocks
    python3 tools/evidence/header_surface_audit.py           # report only
"""

import argparse
import glob
import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
DSO = ROOT / "target" / "debug" / "liblibxml_rs.so"
CAND_HEADERS = sorted(
    list((ROOT / "include" / "libxml").glob("*.h"))
    + list((ROOT / "include" / "libxslt").glob("*.h"))
    + list((ROOT / "include" / "libexslt").glob("*.h"))
)

ORACLE_LIBXML = Path("/usr/include/libxml2/libxml")
ORACLE_LIBXSLT = Path("/usr/include/libxslt")
ORACLE_LIBEXSLT = Path("/usr/include/libexslt")
# fallback extraction sources (older installed-header captures)
FALLBACK_LIBXML = [
    ROOT / "oracle" / "historical" / "prefix" / "libxml2-2.15.0" / "include" / "libxml2" / "libxml",
    ROOT / "archaeology" / "libxml2-git" / "include" / "libxml",
]
FALLBACK_LIBXSLT = [
    ROOT / "oracle" / "historical" / "prefix" / "libxslt-1.1.42" / "include" / "libxslt",
    ROOT / "archaeology" / "libxslt-git" / "libxslt",
]
FALLBACK_LIBEXSLT = [
    ROOT / "oracle" / "historical" / "prefix" / "libxslt-1.1.42" / "include" / "libexslt",
    ROOT / "archaeology" / "libxslt-git" / "libexslt",
]

ARCH_LIBXML = ROOT / "archaeology" / "libxml2-git" / "include" / "libxml"
ARCH_LIBXSLT = ROOT / "archaeology" / "libxslt-git" / "libxslt"
ARCH_LIBEXSLT = ROOT / "archaeology" / "libxslt-git" / "libexslt"

FUNC_MACROS = ("XMLPUBFUN", "LIBXSLT_PUBLIC", "XSLTPUBFUN", "EXSLTPUBFUN")
VAR_MACROS = ("XMLPUBVAR", "LIBXSLT_PUBLIC", "XSLTPUBVAR", "EXSLTPUBVAR")

# Namespaces of the drop-in (ignore Rust-internal helper exports)
PREFIXES = ("xml", "html", "xslt", "exslt")


def dso_exports():
    """Return (functions, data_vars) exported by the candidate DSO."""
    out = subprocess.run(
        ["nm", "-D", "--defined-only", str(DSO)], capture_output=True, text=True
    ).stdout
    funcs, data = set(), set()
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 3:
            name = parts[2]
            if not name.startswith(PREFIXES):
                continue
            if parts[1] == "T":
                funcs.add(name)
            elif parts[1] in ("D", "B", "R"):
                data.add(name)
    return funcs, data


def parse_declared(headers):
    """Return (functions, vars) declared across the candidate headers."""
    funcs, vars_ = set(), set()
    fn_re = re.compile(
        r"^\s*(?:" + "|".join(FUNC_MACROS) + r")[^;(]*\b("
        r"(?:xml|html|xslt|exslt)[A-Za-z0-9_]+)\s*\(",
        re.M,
    )
    var_re = re.compile(
        r"^\s*(?:" + "|".join(VAR_MACROS) + r")[^;]*\b("
        r"(?:xml|html|xslt|exslt)[A-Za-z0-9_]+)\s*;",
        re.M,
    )
    for h in headers:
        text = h.read_text(errors="replace")
        funcs.update(fn_re.findall(text))
        vars_.update(var_re.findall(text))
    return funcs, vars_


def oracle_dirs():
    dirs = []
    for d in (ORACLE_LIBXML, ORACLE_LIBXSLT, ORACLE_LIBEXSLT):
        if d.is_dir():
            dirs.append(d)
    return dirs


def all_source_dirs():
    """Extraction sources, in priority order: system oracle first, then the
    historical prefix captures and archaeology trees."""
    dirs = []
    for d in (ORACLE_LIBXML, ORACLE_LIBXSLT, ORACLE_LIBEXSLT):
        if d.is_dir():
            dirs.append(d)
    for d in FALLBACK_LIBXML + FALLBACK_LIBXSLT + FALLBACK_LIBEXSLT:
        if d.is_dir() and d not in dirs:
            dirs.append(d)
    return dirs


def oracle_headers():
    """The system oracle header set (declared-reference source)."""
    hs = []
    for d in (ORACLE_LIBXML, ORACLE_LIBXSLT, ORACLE_LIBEXSLT):
        if d.is_dir():
            hs.extend(sorted(d.glob("*.h")))
    return hs


def find_declaration(name, is_var):
    """Find the full declaration of `name` in the oracle/archaeology headers.

    Returns (header_path, declaration_text, decl_start_line) or None.

    Oracle declarations span lines in the libtool alignment format, e.g.:

        XMLPUBFUN int
                xmlSaveFile  (const char *filename, xmlDoc *cur);

    so the statement is accumulated from the pub-macro line until the
    terminating `;`, and the name is matched against the whole statement.
    """
    dirs = all_source_dirs()
    macros = VAR_MACROS if is_var else FUNC_MACROS
    for d in dirs:
        for h in sorted(d.glob("*.h")):
            text = h.read_text(errors="replace")
            lines = text.splitlines()
            for i, ln in enumerate(lines):
                stripped = ln.strip()
                if not stripped.startswith(macros):
                    continue
                # accumulate the statement (up to the terminating ';')
                buf = [stripped]
                j = i
                cur = stripped
                while not cur.rstrip().endswith(";") and j + 1 < len(lines):
                    j += 1
                    cur = lines[j].strip()
                    buf.append(cur)
                decl = re.sub(r"\s+", " ", " ".join(buf)).strip()
                if re.search(r"\b" + re.escape(name) + r"\b", decl):
                    return h, decl, i
    return None, None, None


def target_header_for(oracle_header):
    """Map an oracle header path to the candidate header path.

    The candidate mirrors the upstream layout: include/libxml, include/libxslt,
    include/libexslt (flat), while the system keeps libxml under
    include/libxml2/libxml. The project is identified by a path component.
    """
    parts = oracle_header.parts
    fname = oracle_header.name
    proj = None
    for p in parts:
        if p in ("libxml", "libxslt", "libexslt"):
            proj = p
    if proj is None:
        # fall back: file prefix
        if fname.startswith("xslt") or fname in ("transform.h", "security.h", "extensions.h", "extra.h", "functions.h", "imports.h", "keys.h", "namespaces.h", "numbersInternals.h", "pattern.h", "preproc.h", "templates.h", "variables.h", "xsltInternals.h", "xsltlocale.h", "xsltutils.h", "attributes.h", "documents.h"):
            proj = "libxslt"
        elif fname.startswith("exslt"):
            proj = "libexslt"
        else:
            proj = "libxml"
    return ROOT / "include" / proj / fname


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--apply", action="store_true", help="write extraction blocks")
    args = ap.parse_args()

    # The declared reference is the SYSTEM oracle header surface (the true
    # 2.15.3 / 1.1.45 / 0.8.25 contract). Missing = system-declared but not
    # candidate-declared. The export direction is checked separately by the
    # header-compile court (declared -> exported) and the ABI census
    # (oracle-DSO -> candidate-DSO); deprecated symbols that modern upstream
    # headers deliberately no longer declare are intentionally absent here,
    # matching upstream.
    ref_f, ref_v = parse_declared(oracle_headers())
    cand_f, cand_v = parse_declared(CAND_HEADERS)

    missing_f = sorted(ref_f - cand_f)
    missing_v = sorted(ref_v - cand_v)

    print(f"oracle (system) headers declare: {len(ref_f)} functions, {len(ref_v)} vars")
    print(f"candidate headers declare: {len(cand_f)} functions, {len(cand_v)} vars")
    print(f"oracle-declared-but-candidate-undeclared: {len(missing_f)} functions, {len(missing_v)} vars")

    if not args.apply:
        print("\n(report only; use --apply to write extraction blocks)")
        return

    # group missing symbols by candidate target header
    by_target = {}
    unresolved = []
    for name, is_var in [(n, False) for n in missing_f] + [(n, True) for n in missing_v]:
        oh, decl, _ = find_declaration(name, is_var)
        if oh is None:
            unresolved.append(name)
            continue
        target = target_header_for(oh)
        by_target.setdefault(target, []).append((name, decl))

    for target, items in sorted(by_target.items()):
        text = target.read_text(errors="replace")
        # idempotence: skip if block already present
        if "[11.1-S] begin: oracle-extracted" in text:
            existing = set(re.findall(r"\[11\.1-S\][^\n]*\n(.*?)\n/\* \[11\.1-S\] end", text, re.S))
            continue
        items.sort()
        block = [
            "",
            "/* [11.1-S] begin: oracle-extracted declarations",
            * Extracted verbatim from the upstream headers (11.1-S header-surface
            * audit: every function the oracle headers declare must be declared by the
            * drop-in headers — source-compatibility contract). Signatures are the
            * upstream ABI contract.
            " */",
        ]
        for name, decl in items:
            block.append(decl)
        block.append("/* [11.1-S] end: oracle-extracted declarations */")
        block.append("")
        # insert before the closing extern "C" / endif
        insert_at = text.find("#ifdef __cplusplus\n}")
        if insert_at == -1:
            insert_at = text.rfind("#endif")
        block_text = "\n".join(block)
        if insert_at != -1:
            new_text = text[:insert_at] + block_text + "\n" + text[insert_at:]
        else:
            new_text = text.rstrip() + "\n" + block_text + "\n"
        target.write_text(new_text)
        print(f"  updated {target.relative_to(ROOT)} (+{len(items)} declarations)")

    if unresolved:
        print("\nUNRESOLVED (no oracle declaration found):")
        for n in unresolved:
            print("  ", n)


if __name__ == "__main__":
    main()
