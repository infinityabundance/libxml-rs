#!/usr/bin/env python3
"""
symbols.py — ABI ground-truth comparison tool (§14, §9).

Compares header-derived API inventory (from apiatlas.py) against actual
exported symbols from the compiled shared library (DSO), producing a
structured diff of:

  - Functions/globals in headers but NOT exported by the DSO
    (potential omissions, versioned aliases, or private symbols)
  - Functions/globals exported by the DSO but NOT in the headers
    (undocumented exports, legacy ABI, or internal symbols)
  - Globals (B/D) separately classified
  - Versioned symbol aliases

Usage:
  symbols.py libxml2 <version> <dso-path> [--symbols-dir <dir>]
  symbols.py libxslt <version> <dso-path> [--symbols-dir <dir>]
  symbols.py libxml2 2.15.3 /usr/lib/libxml2.so.2
"""

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent


def readelf_symbols(dso_path):
    """Parse exported dynamic symbols from a DSO using readelf.

    Returns two dicts:
      dynamic_functions: {name: {type, binding, version}}
      dynamic_globals:    {name: {type, binding, version}}

    We use readelf -s --wide for detailed info (type, binding, visibility)
    and readelf -d --wide for SONAME/NEEDED.
    """
    result = subprocess.run(
        ["readelf", "-s", "--wide", str(dso_path)],
        capture_output=True, text=True,
    )
    if result.returncode != 0:
        # Fall back to nm -D
        return _nm_symbols(dso_path)

    functions = {}
    globals = {}
    # Symbol version info from readelf --dyn-syms --version-info is complex.
    # We extract versions from the symbol name's @@ suffix instead.
    # readelf -s output format (columns):
    #   Num: Value  Size Type  Bind  Vis  Ndx  Name
    # We parse the type and bind columns.

    for line in result.stdout.splitlines():
        line = line.strip()
        if not line or line.startswith("Symbol table"):
            continue
        parts = line.split()
        if len(parts) < 8:
            continue
        # Try to parse as a symbol table entry
        # Num:Value          Size   Type    Bind      Vis    Ndx Name
        # 0:  0000000000000000 0      NOTYPE  LOCAL  DEFAULT  UND
        # The index column can be numeric or "UND" or "ABS"
        try:
            int(parts[0].rstrip(":"))
        except ValueError:
            continue  # Not a symbol table entry

        sym_type = parts[3]  # FUNC, OBJECT, NOTYPE, TLS, etc.
        bind = parts[4]      # LOCAL, GLOBAL, WEAK
        vis = parts[5]       # DEFAULT, HIDDEN, PROTECTED, INTERNAL
        ndx = parts[6]       # section index: numeric (defined), UND (imported), ABS, COM
        name = parts[7] if len(parts) > 7 else ""

        # We only care about exported symbols: GLOBAL or WEAK binding,
        # DEFAULT or PROTECTED visibility (HIDDEN means not exported).
        if bind not in ("GLOBAL", "WEAK"):
            continue
        if vis not in ("DEFAULT", "PROTECTED"):
            continue

        # CRITICAL: Skip UNDEFINED symbols (imported from glibc, libz, etc.)
        # These are NOT part of libxml2's own ABI.
        if ndx == "UND":
            continue

        # Strip version suffix (@@... or @...)
        bare_name = re.sub(r'(@@?|@).*$', '', name)
        version = name[len(bare_name):] if bare_name != name else ""
        # Strip leading @ from version
        version = version.lstrip("@")

        entry = {
            "type": sym_type,
            "bind": bind,
            "visibility": vis,
            "version": version,
        }

        if sym_type in ("FUNC", "IFUNC"):
            functions[bare_name] = entry
        elif sym_type in ("OBJECT", "TLS", "NOTYPE"):
            # NOTYPE can be globals (e.g. __xmlLastError)
            globals[bare_name] = entry

    return functions, globals


def _nm_symbols(dso_path):
    """Fallback: use nm -D for dynamic symbols.
    
    nm -D shows all dynamic symbols including undefined (imported) ones.
    We skip type 'U' (undefined) to count only symbols defined by this DSO.
    """
    result = subprocess.run(
        ["nm", "-D", str(dso_path)],
        capture_output=True, text=True,
    )
    functions = {}
    globals = {}
    for line in result.stdout.splitlines():
        line = line.strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) < 3:
            continue
        # nm -D format:  <value> <type> <name>
        # type: T/t = text (function), D/d = data (global), B/b = BSS
        # R/r = read-only, U = undefined, N = debug, ? = unknown
        sym_type = parts[1]
        name = parts[2] if len(parts) > 2 else ""

        # Skip undefined (imported from other DSOs)
        if sym_type == "U":
            continue

        # Strip version suffix
        bare_name = re.sub(r'(@@?|@).*$', '', name)
        version = name[len(bare_name):] if bare_name != name else ""
        version = version.lstrip("@")

        entry = {
            "type": sym_type,
            "bind": "GLOBAL",
            "visibility": "DEFAULT",
            "version": version,
        }

        if sym_type in ("T", "t", "W", "w"):
            functions[bare_name] = entry
        elif sym_type in ("D", "d", "B", "b", "R", "r"):
            globals[bare_name] = entry

    return functions, globals


def load_header_symbols(project, version, symbols_dir=None):
    """Load the header-derived symbols record from apiatlas.py output."""
    if symbols_dir is None:
        symbols_dir = ROOT / "atlas" / "symbols"
    path = Path(symbols_dir) / project / f"{version}.json"
    if not path.exists():
        print(f"ERROR: header symbols not found: {path}", file=sys.stderr)
        sys.exit(1)
    with open(path) as f:
        return json.load(f)


def diff_symbols(project, version, dso_path, symbols_dir=None):
    """Compare header-derived symbols against DSO-exported symbols."""
    header = load_header_symbols(project, version, symbols_dir)
    dyn_funcs, dyn_globals = readelf_symbols(dso_path)

    hdr_funcs = set(header.get("functions", []))
    hdr_globals = set(header.get("globals", []))
    dso_funcs = set(dyn_funcs.keys())
    dso_globals = set(dyn_globals.keys())

    # ── Classification helpers ──────────────────────────────────────────

    # System-header leakage: symbols from stdio.h, stdlib.h, etc. that leak
    # into the header inventory because they are declared (or referenced) in
    # libxml2 headers. These are NOT part of the libxml2 API.
    SYSTEM_PREFIXES = (
        "_IO_", "__asprintf", "__fbufsize", "__flbf", "__fpending",
        "__fpurge", "__freadable", "__freading", "__fsetlocking",
        "__ftell", "__fwritable", "__fwriting", "__getdelim",
        "__overflow", "__uflow", "__vprintf_chk",
        "asprintf", "clearerr", "ctermid", "dprintf", "fclose",
        "fdopen", "feof", "ferror", "fflush", "fgetc", "fgetpos",
        "fgets", "fileno", "flockfile", "fmemopen", "fopen",
        "fopencookie", "fprintf", "fputc", "fputs", "fread",
        "free", "freopen", "fscanf", "fseek", "fseeko", "fsetpos",
        "ftell", "ftello", "ftrylockfile", "funlockfile", "fwrite",
        "getc", "getchar", "gets", "open_memstream", "pclose",
        "perror", "printf", "putc", "putchar", "puts", "remove",
        "rename", "rewind", "scanf", "setbuf", "setbuffer",
        "setlinebuf", "setvbuf", "snprintf", "sprintf", "sscanf",
        "stderr", "stdin", "stdout", "strerror", "sys_errlist",
        "sys_nerr", "tempnam", "tmpfile", "tmpnam", "ungetc",
        "vdprintf", "vfprintf", "vprintf", "vsnprintf", "vsprintf",
        "vsscanf",
    )

    # Internal __xml* function declarations: these are actual function
    # declarations in the public headers that serve as the internal
    # implementation behind the public function-pointer variables
    # (xmlFree, xmlMalloc, etc.). They are declared in headers for the
    # macro mechanism but are NOT exported from the DSO.
    # Pattern: __xml* (double underscore prefix) as a FunctionDecl.
    INTERNAL_FUNCTIONS = {
        "__xmlFree",
        "__xmlMalloc",
        "__xmlMallocAtomic",
        "__xmlMemStrdup",
        "__xmlRealloc",
        "__xmlOutputBufferCreateFilename",
    }

    # SAX1 callback names: these appear as function names in readelf output
    # because they are global function-pointer variables (fields of the
    # xmlSAXHandler struct), NOT individual exported functions. The header
    # inventory correctly captures them as fields of the struct, not as
    # individual function declarations.
    SAX1_CALLBACK_NAMES = {
        "internalSubset", "isStandalone", "hasInternalSubset",
        "hasExternalSubset", "resolveEntity", "getEntity",
        "getParameterEntity", "entityDecl", "notationDecl",
        "attributeDecl", "elementDecl", "unparsedEntityDecl",
        "setDocumentLocator", "startDocument", "endDocument",
        "startElement", "endElement", "reference", "characters",
        "ignorableWhitespace", "processingInstruction",
        "comment", "warning", "error", "fatalError",
        "getParameterEntity", "cdataBlock", "externalSubset",
        "initialized",
    }

    # SAX2 callback function exports: these ARE actual exported functions
    # (xmlSAX2*), distinct from the SAX1 callback struct fields.
    # xmlSAX2* functions ARE part of the public API and should be in
    # the header inventory.

    def is_likely_system(name):
        """True if name looks like a C library symbol, not libxml2 API."""
        if name.startswith("xml") or name.startswith("LIBXML") or \
           name.startswith("xslt") or name.startswith("LIBXSLT") or \
           name.startswith("exslt") or name.startswith("__xml") or \
           name.startswith("___xml"):
            return False
        if name.startswith(SYSTEM_PREFIXES):
            return True
        if name in ("optarg", "optind", "opterr", "optopt"):
            return True
        return False

    def is_internal_function(name):
        """True if name is an internal __xml* function not exported."""
        return name in INTERNAL_FUNCTIONS

    def is_sax1_callback(name):
        """True if name is a SAX1 callback struct field, not an individual
        exported function. These appear in DSO symbol tables as OBJECT type
        (global function-pointer variables) but are NOT individual function
        declarations in the headers."""
        return name in SAX1_CALLBACK_NAMES

    # ── Classification ──────────────────────────────────────────────────

    # Separate system-leak symbols
    hdr_funcs_system = {f for f in hdr_funcs if is_likely_system(f)}
    hdr_funcs_internal = {f for f in hdr_funcs if is_internal_function(f)}
    hdr_funcs_clean = {f for f in hdr_funcs
                       if not is_likely_system(f) and not is_internal_function(f)}

    hdr_globals_system = {g for g in hdr_globals if is_likely_system(g)}
    hdr_globals_clean = {g for g in hdr_globals if not is_likely_system(g)}

    # Separate DSO-only symbols into categories
    dso_funcs_sax1 = {f for f in dso_funcs if is_sax1_callback(f)}
    dso_funcs_other = dso_funcs - dso_funcs_sax1

    # Compute diffs (excluding internal and system symbols)
    in_headers_only = hdr_funcs_clean - dso_funcs
    in_dso_only = dso_funcs_other - hdr_funcs_clean
    both = hdr_funcs_clean & dso_funcs_other

    glob_in_headers_only = hdr_globals_clean - dso_globals
    glob_in_dso_only = dso_globals - hdr_globals_clean
    glob_both = hdr_globals_clean & dso_globals

    # Report
    result = {
        "project": project,
        "version": version,
        "dso": str(dso_path),
        "header_source": str(Path(symbols_dir or ROOT / "atlas" / "symbols") / project / f"{version}.json"),
        "summary": {
            "header_functions_total": len(hdr_funcs),
            "header_functions_clean": len(hdr_funcs_clean),
            "header_functions_internal": len(hdr_funcs_internal),
            "header_system_leak": len(hdr_funcs_system),
            "dso_functions_total": len(dso_funcs),
            "dso_functions_sax1": len(dso_funcs_sax1),
            "in_both": len(both),
            "in_headers_only": len(in_headers_only),
            "in_dso_only": len(in_dso_only),
            "header_globals_total": len(hdr_globals),
            "header_globals_clean": len(hdr_globals_clean),
            "header_globals_system_leak": len(hdr_globals_system),
            "dso_globals_total": len(dso_globals),
            "glob_in_both": len(glob_both),
            "glob_in_headers_only": len(glob_in_headers_only),
            "glob_in_dso_only": len(glob_in_dso_only),
        },
        "system_leak_functions": sorted(hdr_funcs_system),
        "system_leak_globals": sorted(hdr_globals_system),
        "internal_functions": sorted(hdr_funcs_internal),
        "sax1_callbacks": sorted(dso_funcs_sax1),
        "in_headers_only_functions": sorted(in_headers_only),
        "in_dso_only_functions": sorted(in_dso_only),
        "in_headers_only_globals": sorted(glob_in_headers_only),
        "in_dso_only_globals": sorted(glob_in_dso_only),
        "dso_functions_with_versions": {
            name: info for name, info in sorted(dyn_funcs.items())
            if info.get("version")
        },
        "dso_globals_with_versions": {
            name: info for name, info in sorted(dyn_globals.items())
            if info.get("version")
        },
    }

    return result


def print_report(result):
    """Print a human-readable comparison report."""
    s = result["summary"]
    print(f"{'='*70}")
    print(f"ABI Ground-Truth Comparison: {result['project']} {result['version']}")
    print(f"{'='*70}")
    print(f"  DSO:          {result['dso']}")
    print(f"  Header src:   {result['header_source']}")
    print(f"\n  {'Functions':<30} {'Headers':>8} {'DSO':>8} {'Both':>8} {'H-only':>8} {'DSO-only':>8}")
    print(f"  {'─'*70}")
    print(f"  {'Total':<30} {s['header_functions_total']:>8} {s['dso_functions_total']:>8} "
          f"{s['in_both']:>8} {s['in_headers_only']:>8} {s['in_dso_only']:>8}")
    print(f"  {'Clean (w/o system leak)':<30} {s['header_functions_clean']:>8}")
    print(f"  {'System leak filtered':<30} {s['header_system_leak']:>8}")
    print(f"\n  {'Globals':<30} {'Headers':>8} {'DSO':>8} {'Both':>8} {'H-only':>8} {'DSO-only':>8}")
    print(f"  {'─'*70}")
    print(f"  {'Total':<30} {s['header_globals_total']:>8} {s['dso_globals_total']:>8} "
          f"{s['glob_in_both']:>8} {s['glob_in_headers_only']:>8} {s['glob_in_dso_only']:>8}")
    print(f"  {'Clean (w/o system leak)':<30} {s['header_globals_clean']:>8}")
    print(f"  {'System leak filtered':<30} {s['header_globals_system_leak']:>8}")

    if result["system_leak_functions"]:
        print(f"\n  System-leak functions ({len(result['system_leak_functions'])}):")
        for fn in result["system_leak_functions"][:20]:
            print(f"    {fn}")
        if len(result["system_leak_functions"]) > 20:
            print(f"    ... and {len(result['system_leak_functions']) - 20} more")

    if result["internal_functions"]:
        print(f"\n  Internal __xml* functions (in headers, not exported) ({len(result['internal_functions'])}):")
        for fn in result["internal_functions"]:
            print(f"    {fn}")

    if result["sax1_callbacks"]:
        print(f"\n  SAX1 callback struct fields (in DSO, not individual functions) ({len(result['sax1_callbacks'])}):")
        for fn in list(result["sax1_callbacks"])[:15]:
            print(f"    {fn}")
        if len(result["sax1_callbacks"]) > 15:
            print(f"    ... and {len(result['sax1_callbacks']) - 15} more")

    if result["in_headers_only_functions"]:
        print(f"\n  IN HEADERS ONLY (not in DSO) ({len(result['in_headers_only_functions'])}):")
        for fn in result["in_headers_only_functions"][:30]:
            print(f"    {fn}")
        if len(result["in_headers_only_functions"]) > 30:
            print(f"    ... and {len(result['in_headers_only_functions']) - 30} more")

    if result["in_dso_only_functions"]:
        print(f"\n  IN DSO ONLY (not in headers) ({len(result['in_dso_only_functions'])}):")
        for fn in result["in_dso_only_functions"][:30]:
            print(f"    {fn}")
        if len(result["in_dso_only_functions"]) > 30:
            print(f"    ... and {len(result['in_dso_only_functions']) - 30} more")

    if result["in_headers_only_globals"]:
        print(f"\n  IN HEADERS ONLY (globals) ({len(result['in_headers_only_globals'])}):")
        for g in result["in_headers_only_globals"][:20]:
            print(f"    {g}")
        if len(result["in_headers_only_globals"]) > 20:
            print(f"    ... and {len(result['in_headers_only_globals']) - 20} more")

    if result["in_dso_only_globals"]:
        print(f"\n  IN DSO ONLY (globals) ({len(result['in_dso_only_globals'])}):")
        for g in result["in_dso_only_globals"][:20]:
            print(f"    {g}")
        if len(result["in_dso_only_globals"]) > 20:
            print(f"    ... and {len(result['in_dso_only_globals']) - 20} more")

    if result["dso_functions_with_versions"]:
        print(f"\n  Versioned function symbols ({len(result['dso_functions_with_versions'])}):")
        shown = 0
        for name, info in list(result["dso_functions_with_versions"].items())[:15]:
            print(f"    {name}  [{info['version']}]")
            shown += 1
        if len(result["dso_functions_with_versions"]) > 15:
            print(f"    ... and {len(result['dso_functions_with_versions']) - shown} more")

    print(f"\n{'='*70}")


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(1)

    project = sys.argv[1]
    version = sys.argv[2]
    dso_path = sys.argv[3]

    symbols_dir = None
    if len(sys.argv) > 4 and sys.argv[4] != "--":
        symbols_dir = sys.argv[4]

    if not os.path.isfile(dso_path):
        print(f"ERROR: DSO not found: {dso_path}", file=sys.stderr)
        sys.exit(1)

    result = diff_symbols(project, version, dso_path, symbols_dir)
    print_report(result)

    # Also write structured output
    out_dir = ROOT / "atlas" / "abi"
    out_dir.mkdir(parents=True, exist_ok=True)
    out_path = out_dir / f"{project}-{version}-abi-diff.json"
    with open(out_path, "w") as f:
        json.dump(result, f, indent=2, sort_keys=True)
    print(f"\nStructured ABI diff written to: {out_path}")


if __name__ == "__main__":
    main()
