#!/usr/bin/env python3
"""
apiatlas.py — Public API surface extractor using clang AST.

Phase 0 / §9 / §13 deliverable generator.

For a given upstream project + version tag, this tool:
  1. Checks out the public headers from the pinned git tree (archaeology/*-git)
  2. Runs clang AST dump (JSON) over each header
  3. Extracts public declarations: functions, typedefs, structs, unions,
     enums+values, globals, macros (via preprocessed output), callbacks
  4. Emits machine-readable records:
       atlas/api/<project>/<version>.json
       atlas/symbols/<project>/<version>.json
  5. Optionally compares against a previously recorded version (delta)

The machine-readable JSON is canonical; human reports derive from it (§9).

Usage:
  apiatlas.py libxml2 <version> <git-tag> [--outdir DIR] [--cc clang]
  apiatlas.py libxslt <version> <git-tag>
  apiatlas.py libxml2 v2.15.3 v2.15.3
"""

import json
import os
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent

# Import sibling tooling for generated-config reconstruction.
sys.path.insert(0, str(Path(__file__).resolve().parent))
import profileconfig  # noqa: E402

GIT_DIRS = {
    "libxml2": ROOT / "archaeology" / "libxml2-git",
    "libxslt": ROOT / "archaeology" / "libxslt-git",
    # libexslt ships inside the libxslt tree (libexslt/); its version
    # (0.8.x) has no separate tags, so the caller passes the libxslt tag
    # that shipped the matching exslt release (0.8.25 == v1.1.45).
    "libexslt": ROOT / "archaeology" / "libxslt-git",
}

# Header subdirectories within the git tree, per project.
HEADER_DIRS = {
    "libxml2": ["include/libxml"],
    "libxslt": ["libxslt"],
    "libexslt": ["libexslt"],
}

# Headers that are internal/private and should not contribute to the public
# API inventory (they leak into the include dir but are not installed/public).
PRIVATE_HEADERS = {
    "libxml2": {
        "schemasInternals.h",   # installed but semi-private (xsd internals)
        "xmlversion.h.in",      # configure template, not a real header
        "Makefile.am", "meson.build",
    },
    "libxslt": {
        "triodef.h", "trio.h",  # bundled trio (printf impl) internals
        "win32config.h",        # windows build config
    },
    "libexslt": {
        "exsltconfig.h.in",     # configure template, not a real header
        "Makefile.am",
    },
}

# Visibility-marker macros handled by the preprocessor (they resolve to
# __attribute__((visibility(...))) or nothing depending on build).
# We rely on clang to preprocess these; nothing to do here, but we record
# which headers were scanned and which were skipped as private.

EXPORT_KINDS = {"FunctionDecl", "VarDecl"}

# System/C library function names that leak into libxml2 headers via #include
# or direct declarations. These are NOT part of the libxml2 API but appear in
# the translation unit because libxml2 headers include <stdio.h>, <stdlib.h>,
# <string.h>, etc. or declare them directly for portability reasons.
#
# We filter these by name at extraction time because clang's AST may not
# report their origin correctly (line=None, includedFrom fallback).
# This is a secondary defense after the origin-based filtering.
SYSTEM_FUNCTION_NAMES = frozenset({
    # stdio.h
    "clearerr", "fclose", "feof", "ferror", "fflush", "fgetc", "fgetpos",
    "fgets", "fileno", "fmemopen", "fopen", "fopencookie", "fprintf",
    "fputc", "fputs", "fread", "freopen", "fscanf", "fseek", "fseeko",
    "fsetpos", "ftell", "ftello", "fwrite", "getc", "getchar", "gets",
    "open_memstream", "pclose", "perror", "popen", "printf", "putc",
    "putchar", "puts", "remove", "rename", "renameat", "rewind", "scanf",
    "setbuf", "setbuffer", "setlinebuf", "setvbuf", "snprintf", "sprintf",
    "sscanf", "stderr", "stdin", "stdout", "tempnam", "tmpfile", "tmpnam",
    "ungetc", "vdprintf", "vfprintf", "vprintf", "vsnprintf", "vsprintf",
    "vsscanf", "asprintf", "vasprintf", "dprintf", "getdelim", "getline",
    "getw", "putw", "getc_unlocked", "getchar_unlocked", "putc_unlocked",
    "putchar_unlocked", "feof_unlocked", "ferror_unlocked",
    "fflush_unlocked", "fgetc_unlocked", "fputc_unlocked", "fread_unlocked",
    "fwrite_unlocked", "flockfile", "ftrylockfile", "funlockfile",
    "fileno_unlocked", "fgetpos64", "fsetpos64", "fopen64", "tmpnam_r",
    # stdlib.h
    "free", "malloc", "realloc", "calloc", "abort", "exit", "atexit",
    "getenv", "system", "qsort", "bsearch", "abs", "labs", "div", "ldiv",
    "rand", "srand", "mblen", "mbtowc", "wctomb", "mbstowcs", "wcstombs",
    # string.h
    "strlen", "strcpy", "strncpy", "strcat", "strncat", "strcmp",
    "strncmp", "strchr", "strrchr", "strstr", "strspn", "strcspn",
    "strpbrk", "strtok", "memset", "memcpy", "memmove", "memcmp",
    "memchr", "strerror", "strdup", "strndup",
    # ctype.h
    "isalnum", "isalpha", "isblank", "iscntrl", "isdigit", "isgraph",
    "islower", "isprint", "ispunct", "isspace", "isupper", "isxdigit",
    "tolower", "toupper",
    # errno.h
    "errno",
    # signal.h
    "signal", "raise",
    # time.h
    "time", "clock", "difftime", "mktime", "asctime", "ctime",
    "gmtime", "localtime", "strftime",
    # locale.h
    "setlocale", "localeconv",
    # math.h (commonly pulled in)
    "sin", "cos", "tan", "asin", "acos", "atan", "atan2",
    "sinh", "cosh", "tanh", "exp", "log", "log10", "pow",
    "sqrt", "ceil", "floor", "fabs", "ldexp", "frexp", "modf",
    "fmod",
    # unistd.h / POSIX
    "read", "write", "open", "close", "lseek", "dup", "dup2",
    "pipe", "fork", "exec", "execve", "execlp", "execvp",
    "getpid", "getppid", "sleep", "usleep", "access", "chdir",
    "getcwd", "stat", "lstat", "fstat", "unlink", "link", "symlink",
    "mkdir", "rmdir", "chmod", "chown", "getuid", "getgid",
    # sys/types.h
    "size_t", "ssize_t", "off_t", "pid_t", "uid_t", "gid_t",
    # sys/stat.h
    "umask",
    # fcntl.h
    "fcntl", "creat",
    # dlfcn.h
    "dlopen", "dlclose", "dlsym", "dlerror",
    # __-prefixed internal glibc symbols (leak through clang built-in decls)
    "__asprintf", "__getdelim", "__overflow", "__uflow",
    "__isoc99_vfscanf", "__isoc99_vscanf", "__isoc99_fscanf", "__isoc99_scanf", "__isoc99_sscanf",
    # POSIX/glibc extensions that leak from system headers
    "clearerr_unlocked", "ctermid", "fdopen",
    # scanf variants missing from the main list
    "vfscanf", "vscanf",
    "feof_unlocked", "ferror_unlocked", "fflush_unlocked",
    "fgetc_unlocked", "fputc_unlocked", "fread_unlocked",
    "fwrite_unlocked", "putc_unlocked", "putchar_unlocked",
    "__fbufsize", "__flbf", "__fpending", "__fpurge",
    "__freadable", "__freading", "__fsetlocking",
    "__ftell", "__fwritable", "__fwriting",
    "__vprintf_chk", "__vsprintf_chk", "__vfprintf_chk",
    "__snprintf_chk", "__vsnprintf_chk",
    # iconv.h
    "iconv", "iconv_open", "iconv_close",
    # libz.h
    "compress", "uncompress", "gzopen", "gzdopen", "gzread",
    "gzwrite", "gzclose", "gzgets", "gzputs",
})


def git_archive(project, tag, subdirs, dest):
    gd = GIT_DIRS[project]
    dest = Path(dest)
    dest.mkdir(parents=True, exist_ok=True)
    for sub in subdirs:
        out = subprocess.run(
            ["git", "-C", str(gd), "archive", tag, sub],
            check=True, capture_output=True,
        ).stdout
        t = subprocess.run(["tar", "-x", "-C", str(dest)], input=out, capture_output=True)
        if t.returncode != 0:
            print(f"tar error for {sub}: {t.stderr.decode()}", file=sys.stderr)


def walk(nodes, cb, depth=0):
    """Recursively visit AST JSON nodes."""
    for n in nodes:
        if not isinstance(n, dict):
            continue
        cb(n)
        walk(n.get("inner", []), cb, depth + 1)


def extract(project, version, tag, cc="clang", tmpdir=None):
    """Extract API surface from a version tag. Returns a dict record."""
    subdirs = HEADER_DIRS[project]
    dest = Path(tmpdir) if tmpdir else Path(tempfile.mkdtemp(prefix=f"apiatlas-{project}-"))
    git_archive(project, tag, subdirs, dest)

    # Reconstruct generated configuration headers (xmlversion.h for libxml2,
    # xsltconfig.h for libxslt) so the header set is self-contained. This is
    # required because the public headers include the generated file, which is
    # only produced by the build system (§9 item 18, §56).
    profile = "distro"
    gen_meta = {}
    if project == "libxml2":
        gen_meta = profileconfig.generate(
            project, version, tag, profile, dest / "include" / "libxml")
    elif project == "libexslt":
        # libexslt ships inside the libxslt tree; its config header template
        # is libexslt/exsltconfig.h.in (generated into the same dir).
        gen_meta = profileconfig.generate(
            project, version, tag, profile, dest / "libexslt")
    else:
        gen_meta = profileconfig.generate(
            project, version, tag, profile, dest / "libxslt")

    hdr_root = dest / subdirs[0]
    # Headers include each other as <libxml/...> (angle-bracket with the
    # libxml/ prefix), so -I must point at the PARENT of the libxml/ dir.
    # For libxslt we also need the libxml headers on the include path because
    # libxslt headers include <libxml/...>. libexslt needs both libxml2 and
    # libxslt headers (exslt.h includes <libxml/tree.h> and <libxml/xpath.h>;
    # libexslt.h includes <libxslt/xsltconfig.h>).
    inc_dirs = [str(hdr_root.parent)]
    if project in ("libxslt", "libexslt"):
        # Reuse a libxml2 header export from the sibling git tree at a modern tag.
        libxml_tag = "v2.15.3"
        libxml_dest = dest / "libxml-includes"
        git_archive("libxml2", libxml_tag, ["include/libxml"], libxml_dest)
        # The libxml2 headers include the generated <libxml/xmlversion.h>;
        # generate it into the helper tree too (distro profile).
        profileconfig.generate("libxml2", "2.15.3", libxml_tag, "distro",
                               libxml_dest / "include" / "libxml")
        inc_dirs.append(str(libxml_dest / "include"))
    if project == "libexslt":
        # libexslt.h includes <libxslt/xsltconfig.h> and <libxml/xmlversion.h>
        xslt_tag = tag
        xslt_dest = dest / "libxslt-includes"
        git_archive("libxslt", xslt_tag, ["libxslt"], xslt_dest)
        profileconfig.generate("libxslt", "1.1.45", xslt_tag, "distro",
                               xslt_dest / "libxslt")
        inc_dirs.append(str(xslt_dest))
    inc_args = []
    for d in inc_dirs:
        inc_args += ["-I", d]

    # Headers with circular dependency workarounds need defines to expose
    # their actual declarations. When processing tree.h directly, we must
    # define XML_TREE_INTERNALS so the real tree declarations are visible
    # (otherwise tree.h just includes parser.h as a circular-dependency
    # workaround). Other headers (entities.h, valid.h, xmlIO.h, parser.h)
    # already define this before including tree.h, so adding it globally
    # is harmless and matches the pattern used throughout libxml2.
    inc_args += ["-DXML_TREE_INTERNALS"]

    headers = sorted(p for p in hdr_root.rglob("*.h")
                     if p.name not in PRIVATE_HEADERS.get(project, set()))

    record = {
        "project": project,
        "version_tag": tag,
        "generator": "tools/archaeology/apiatlas.py",
        "headers": [],
        "functions": [],
        "typedefs": [],
        "records": [],     # struct/union
        "enums": [],       # enum + values
        "enumerators": [],
        "globals": [],
        "callbacks": [],   # function-pointer typedefs
    }
    seen_decl = set()

    # Build a set of known libxml2/libxslt header file paths for origin checking.
    # This is used because clang's JSON AST for function declarations from
    # included files only reports the includer (via loc.includedFrom), not the
    # actual declaration file. We use #line directives from -E output to map
    # line numbers to actual source files.
    hdr_root_str = str(hdr_root)

    for hdr in headers:
        rel = hdr.relative_to(hdr_root)
        hinfo = {"header": str(rel), "declarations": []}
        # Preprocess with clang -E (keep #line directives for origin tracking).
        # We use -E without -P to keep #line directives, which let us map
        # line numbers in the preprocessed output back to original source files.
        pp = subprocess.run(
            [cc, "-E", *inc_args, str(hdr)],
            capture_output=True, text=True,
        )
        if pp.returncode != 0:
            hinfo["preprocess_error"] = pp.stderr
            record["headers"].append(hinfo)
            continue

        # Build line -> file mapping from #line directives in preprocessed output.
        # Format: # line-number "filename" flags
        #
        # CRITICAL: The line-number in the #line directive is the ORIGINAL SOURCE
        # line number, NOT the preprocessed output line number. However, clang's
        # AST reports loc.line in the PREPROCESSED OUTPUT space for function
        # declarations from included files. So we use the ACTUAL preprocessed
        # output line number (the enumerate index) as the mapping key, which is
        # always unique — preventing the overwriting bug that occurred when
        # multiple #line directives shared the same original source line number.
        #
        # The filenames in #line directives may be relative (to the source file's
        # directory or the working directory) or absolute. We resolve them against
        # the header file's parent directory.
        hdr_dir = str(hdr.parent)
        # List of (preprocessed_output_line, filename) transitions, in order.
        # Preprocessed output line number is 1-indexed (the line position in -E output).
        transitions = []
        for i, pp_line in enumerate(pp.stdout.splitlines(), start=1):
            m = re.match(r'^#\s+(\d+)\s+"([^"]+)"(.*)$', pp_line)
            if m:
                filename = m.group(2)
                # Resolve relative paths against the header's parent directory.
                if not os.path.isabs(filename):
                    resolved = os.path.normpath(os.path.join(hdr_dir, filename))
                else:
                    resolved = filename
                # i is the ACTUAL preprocessed output line number (unique per line)
                transitions.append((i, resolved))

        # AST dump as JSON
        ast = subprocess.run(
            [cc, "-Xclang", "-ast-dump=json", "-fsyntax-only", *inc_args, str(hdr)],
            capture_output=True, text=True,
        )
        if ast.returncode != 0:
            hinfo["ast_error"] = ast.stderr[:2000]
            record["headers"].append(hinfo)
            continue

        try:
            tree = json.loads(ast.stdout)
        except json.JSONDecodeError as e:
            hinfo["ast_parse_error"] = str(e)
            record["headers"].append(hinfo)
            continue

        def resolve_origin(n, default_hdr):
            """Resolve the actual source file for a declaration.

            Clang's JSON AST reports location differently depending on
            where the declaration lives:

            Pattern A — Decl directly in the main file (the header being
            processed):
              loc = { "line": N, "col": C, "offset": O }
              NO "file" field, NO "includedFrom" field.
              → Return default_hdr (the header being processed).

            Pattern B — Decl from an included file (system header or
            another libxml2 header):
              loc = { "includedFrom": { "file": ..., ... }, "line": N }
              Has "includedFrom" but NO "file" field.
              → Return None (will be filtered out; captured when its
                own header is processed if it's a libxml2 header).

            Pattern C — Type declarations (typedef, struct, enum) from
            included files:
              loc = { "file": "/path/to/actual/file.h", "line": N }
              HAS a "file" field pointing to the actual source.
              → Return the file path directly.
            """
            loc = n.get("loc", {})

            # Pattern C: explicit file field (type decls from included files).
            if "file" in loc:
                return str(loc["file"])
            # Also check range.begin.file
            rng = n.get("range", {}).get("begin", {})
            if "file" in rng:
                return str(rng["file"])

            # Pattern B: has includedFrom — declaration is from an included
            # file (system header or other libxml2 header). We cannot
            # determine the actual file reliably, so filter it out.
            # It will be captured when its own header is processed directly.
            if "includedFrom" in loc:
                return None
            if "includedFrom" in rng:
                return None

            # Pattern A: directly in the main file — return the default
            # header path. This is the common case for public API functions.
            return str(default_hdr)

        def collect(n):
            kind = n.get("kind")
            # Resolve the actual origin file for this declaration
            origin = resolve_origin(n, hdr)
            # Keep only declarations living in the extracted header tree.
            # origin=None means the declaration is from an included file and
            # we cannot determine its origin reliably — filter it out.
            # It will be captured when its own header is processed directly.
            if origin is None:
                return
            if not origin.startswith(hdr_root_str):
                return
            # Safety net: also filter declarations from known system/include paths.
            # This catches cases where the #line mapping or fallback resolves to
            # a system header path that happens to not start with hdr_root_str
            # (though the above check should catch most). It also catches
            # <built-in> or /usr/lib/clang/ paths from clang's implicit includes.
            _system_prefixes = ("/usr/include/", "/usr/lib/", "<built-in>",
                                "/usr/local/include/", "/usr/lib/clang/")
            if origin.startswith(_system_prefixes):
                return

            if kind in ("FunctionDecl", "TypedefDecl", "RecordDecl", "EnumDecl",
                        "VarDecl", "EnumConstantDecl"):
                name = n.get("name")

                # Name-based system function filter: catch C library functions
                # that leak through because clang's AST reports no line number
                # (line=None), bypassing the #line mapping, and the includedFrom
                # fallback points to the libxml2 header. These are known C
                # standard library and POSIX function names that are NOT part
                # of the libxml2 API.
                if name and name in SYSTEM_FUNCTION_NAMES:
                    return
                # Anonymous enums (typedef enum {...} name;) have no name on
                # the EnumDecl; the real name is on the following typedef.
                # Synthesize a stable key from the source location so we can
                # link them in a post-pass.
                is_anon_enum = (kind == "EnumDecl" and not name)
                if not name and not is_anon_enum:
                    return
                line = n.get("loc", {}).get("line")
                col = n.get("loc", {}).get("col")
                key = (kind, name or f"__anon_enum_{line}_{col}")
                if key in seen_decl:
                    return
                seen_decl.add(key)

                entry = {
                    "kind": kind,
                    "name": name,
                    "type": n.get("type", {}).get("qualType"),
                    "line": line,
                    "col": col,
                    "header": str(rel),
                }
                # Function: capture params
                if kind == "FunctionDecl":
                    params = []
                    for c in n.get("inner", []):
                        if c.get("kind") == "ParmVarDecl":
                            params.append({
                                "name": c.get("name"),
                                "type": c.get("type", {}).get("qualType"),
                            })
                    entry["params"] = params
                    entry["storage"] = n.get("storageClass")
                    entry["isVariadic"] = n.get("variadic", False)
                    record["functions"].append(entry)
                if kind == "RecordDecl":
                    entry["tagUsed"] = n.get("tagUsed")
                    entry["completeDefinition"] = n.get("completeDefinition", False)
                    fields = []
                    for c in n.get("inner", []):
                        if c.get("kind") == "FieldDecl":
                            fields.append({
                                "name": c.get("name"),
                                "type": c.get("type", {}).get("qualType"),
                            })
                    entry["fields"] = fields
                    record["records"].append(entry)
                elif kind == "EnumDecl":
                    values = []
                    for c in n.get("inner", []):
                        if c.get("kind") == "EnumConstantDecl":
                            values.append({
                                "name": c.get("name"),
                                "value": c.get("value", c.get("init")),
                                "type": c.get("type", {}).get("qualType"),
                            })
                            record["enumerators"].append({
                                "enum": name or entry["name"],
                                "enum_line": line,
                                "name": c.get("name"),
                                "value": c.get("value", c.get("init")),
                            })
                    entry["values"] = values
                    entry["anonymous"] = is_anon_enum
                    record["enums"].append(entry)
                elif kind == "VarDecl":
                    record["globals"].append(entry)
                elif kind == "TypedefDecl":
                    record["typedefs"].append(entry)
                    # function-pointer typedefs are callbacks
                    q = (n.get("type", {}).get("qualType") or "")
                    if "(*" in q:
                        record["callbacks"].append(entry)
                # record into per-header listing
                hinfo["declarations"].append({
                    "kind": kind, "name": name,
                    "type": n.get("type", {}).get("qualType"),
                })

        walk([tree], collect)
        record["headers"].append(hinfo)

    # Derived counts
    # Post-pass: link anonymous enums to their typedef names. In C,
    # `typedef enum {...} xmlElementType;` produces an anonymous EnumDecl
    # followed (in the same header, adjacent lines) by a TypedefDecl whose
    # type is that enum. We associate each anonymous enum with the nearest
    # typedef at the same header/line range.
    anon_enums = [e for e in record["enums"] if e.get("anonymous")]
    if anon_enums:
        # Build (header, line) -> typedef name map from the collected typedefs.
        typedef_lines = {}
        for td in record["typedefs"]:
            key = (td.get("header"), td.get("line"))
            typedef_lines[key] = td["name"]
        for e in anon_enums:
            hdr = e.get("header")
            ln = e.get("line")
            # typedef usually lands on the same line (closing brace + name)
            # or the next line.
            linked = None
            for cand in (ln, (ln or 0) + 1, (ln or 0) + 2, (ln or 0) - 1):
                if cand and (hdr, cand) in typedef_lines:
                    linked = typedef_lines[(hdr, cand)]
                    break
            e["typedef_name"] = linked
            if linked:
                e["name"] = linked

    record["summary"] = {
        "functions": len(record["functions"]),
        "typedefs": len(record["typedefs"]),
        "records": len(record["records"]),
        "enums": len(record["enums"]),
        "enumerators": len(record["enumerators"]),
        "globals": len(record["globals"]),
        "callbacks": len(record["callbacks"]),
        "headers": len(record["headers"]),
    }
    return record


def write_records(project, version, record, outdir):
    outdir = Path(outdir)
    api_dir = outdir / "api" / project
    sym_dir = outdir / "symbols" / project
    api_dir.mkdir(parents=True, exist_ok=True)
    sym_dir.mkdir(parents=True, exist_ok=True)

    with open(api_dir / f"{version}.json", "w") as f:
        json.dump(record, f, indent=2, sort_keys=True)

    # Symbols record: exported functions + globals (the ABI surface).
    symbols = {
        "project": project,
        "version": version,
        "version_tag": record["version_tag"],
        "functions": sorted({f["name"] for f in record["functions"]}),
        "globals": sorted({g["name"] for g in record["globals"]}),
    }
    with open(sym_dir / f"{version}.json", "w") as f:
        json.dump(symbols, f, indent=2, sort_keys=True)

    return api_dir / f"{version}.json", sym_dir / f"{version}.json"


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(1)
    project = sys.argv[1]
    version = sys.argv[2]
    tag = sys.argv[3]
    outdir = Path(sys.argv[4]) if len(sys.argv) > 4 and sys.argv[4] != "--" else ROOT / "atlas"
    record = extract(project, version, tag)
    api_path, sym_path = write_records(project, version, record, outdir)
    s = record["summary"]
    print(f"{project} {version} ({tag}): "
          f"{s['functions']} functions, {s['typedefs']} typedefs, "
          f"{s['records']} records, {s['enums']} enums ({s['enumerators']} enumerators), "
          f"{s['globals']} globals, {s['callbacks']} callbacks, {s['headers']} headers")
    print(f"  api:     {api_path}")
    print(f"  symbols: {sym_path}")


if __name__ == "__main__":
    main()
