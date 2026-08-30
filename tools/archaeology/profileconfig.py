#!/usr/bin/env python3
"""
profileconfig.py — Reconstruct generated configuration headers from templates.

Phase 0 / §9 (item 18: capture generated configuration), §56 (build-feature
archaeology), §15 (candidate headers).

libxml2's public headers are NOT self-contained: include/libxml/xmlversion.h is
generated at build time from xmlversion.h.in by configure/cmake. The same is
true for libxslt (libxslt/xsltconfig.h from xsltconfig.h.in). To (a) extract
the public API with clang and (b) ship compatible candidate headers, we must
reconstruct these generated files from a chosen feature profile.

The default profile ('distro') enables the features a typical modern distro
build ships — the primary compatibility target. Other profiles encode
historical/configuration variants per §56.

Usage:
  profileconfig.py libxml2 <version> <tag> [--profile distro] [--out DIR]
  profileconfig.py libxslt <version> <tag> [--profile distro] [--out DIR]
"""

import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
GIT_DIRS = {
    "libxml2": ROOT / "archaeology" / "libxml2-git",
    "libxslt": ROOT / "archaeology" / "libxslt-git",
    # libexslt ships inside the libxslt tree
    "libexslt": ROOT / "archaeology" / "libxslt-git",
}

# Templates (path within the git tree).
TEMPLATES = {
    "libxml2": "include/libxml/xmlversion.h.in",
    "libxslt": "libxslt/xsltconfig.h.in",
    "libexslt": "libexslt/exsltconfig.h.in",
}

# Feature profiles. Each maps the @WITH_*@ placeholder to a value.
# '1' means enabled, '0' disabled. For libxml2, WITH_THREADS/SAX1 etc. follow.
PROFILES = {
    "distro": {
        "libxml2": {
            # typical modern distro: everything the distro builds in
            "WITH_THREADS": 1,
            "WITH_THREAD_ALLOC": 1,
            "WITH_TREE": 1,
            "WITH_OUTPUT": 1,
            "WITH_PUSH": 1,
            "WITH_READER": 1,
            "WITH_PATTERN": 1,
            "WITH_WRITER": 1,
            "WITH_SAX1": 1,
            "WITH_HTTP": 1,
            "WITH_VALID": 1,
            "WITH_HTML": 1,
            "WITH_LEGACY": 1,
            "WITH_C14N": 1,
            "WITH_CATALOG": 1,
            "WITH_XPATH": 1,
            "WITH_XPTR": 1,
            "WITH_XINCLUDE": 1,
            "WITH_ICONV": 1,
            "WITH_ICU": 0,
            "WITH_ISO8859X": 1,
            "WITH_DEBUG": 1,
            "WITH_REGEXPS": 1,
            "WITH_RELAXNG": 1,
            "WITH_SCHEMAS": 1,
            "WITH_SCHEMATRON": 1,
            "WITH_MODULES": 1,
            "WITH_ZLIB": 1,
            "WITH_LZMA": 1,
        },
        "libxslt": {
            "WITH_DEBUGGER": 1,
            "WITH_MODULES": 1,
            "WITH_PROFILER": 1,
            "WITH_TRIO": 1,
            "WITH_XSLT_DEBUG": 1,
        },
        "libexslt": {
            "WITH_EXSLT_DEBUG": 1,
            "WITH_CRYPTO": 1,
        },
    },
    # Minimal profile for probing compile-against behavior of minimal builds.
    "minimal": {
        "libxml2": {
            "WITH_THREADS": 0, "WITH_THREAD_ALLOC": 0, "WITH_TREE": 1,
            "WITH_OUTPUT": 0, "WITH_PUSH": 0, "WITH_READER": 0,
            "WITH_PATTERN": 0, "WITH_WRITER": 0, "WITH_SAX1": 0,
            "WITH_HTTP": 0, "WITH_VALID": 0, "WITH_HTML": 0,
            "WITH_LEGACY": 0, "WITH_C14N": 0, "WITH_CATALOG": 0,
            "WITH_XPATH": 0, "WITH_XPTR": 0, "WITH_XINCLUDE": 0,
            "WITH_ICONV": 0, "WITH_ICU": 0, "WITH_ISO8859X": 0,
            "WITH_DEBUG": 0, "WITH_REGEXPS": 0, "WITH_RELAXNG": 0,
            "WITH_SCHEMAS": 0, "WITH_SCHEMATRON": 0, "WITH_MODULES": 0,
            "WITH_ZLIB": 0, "WITH_LZMA": 0,
        },
        "libxslt": {
            "WITH_DEBUGGER": 0, "WITH_MODULES": 0, "WITH_PROFILER": 0,
            "WITH_TRIO": 0, "WITH_XSLT_DEBUG": 0,
        },
        "libexslt": {
            "WITH_EXSLT_DEBUG": 0,
            "WITH_CRYPTO": 0,
        },
    },
}


def fetch_template(project, tag):
    gd = GIT_DIRS[project]
    tpl = TEMPLATES[project]
    out = subprocess.run(
        ["git", "-C", str(gd), "show", f"{tag}:{tpl}"],
        capture_output=True, text=True,
    )
    if out.returncode != 0:
        raise SystemExit(f"cannot read {tpl} at {tag}: {out.stderr}")
    return out.stdout


def generate(project, version, tag, profile, outdir):
    template = fetch_template(project, tag)
    with_options = PROFILES[profile][project]

    # Special substitutions that aren't @WITH_*@
    special = {}
    if project == "libxml2":
        special["VERSION"] = version
        # LIBXML_VERSION_NUMBER is like 21503 (major*10000+minor*100+micro)
        parts = version.split(".")
        maj, minor, micro = (int(parts[0]), int(parts[1]),
                             int(parts[2]) if len(parts) > 2 else 0)
        special["LIBXML_VERSION_NUMBER"] = maj * 10000 + minor * 100 + micro
        special["LIBXML_VERSION_EXTRA"] = ""
        special["MODULE_EXTENSION"] = ".so"
    elif project == "libxslt":
        special["VERSION"] = version
        parts = version.split(".")
        maj, minor, micro = (int(parts[0]), int(parts[1]),
                             int(parts[2]) if len(parts) > 2 else 0)
        special["LIBXSLT_VERSION_NUMBER"] = maj * 10000 + minor * 100 + micro
        special["LIBXSLT_VERSION_EXTRA"] = ""
        special["LIBXSLT_DEFAULT_PLUGINS_PATH"] = "/usr/lib/libxslt-plugins"
    elif project == "libexslt":
        special["VERSION"] = version
        special["LIBEXSLT_VERSION"] = version
        parts = version.split(".")
        maj, minor, micro = (int(parts[0]), int(parts[1]),
                             int(parts[2]) if len(parts) > 2 else 0)
        special["LIBEXSLT_VERSION_NUMBER"] = maj * 10000 + minor * 100 + micro
        special["LIBEXSLT_VERSION_EXTRA"] = ""
        special["LIBEXSLT_DOTTED_VERSION"] = version

    def repl(m):
        key = m.group(1)
        if key in special:
            return str(special[key])
        if key in with_options:
            return str(with_options[key])
        # Unknown placeholder: fail loudly rather than silently generating a
        # broken header (no silent omissions, §99).
        raise SystemExit(f"unknown placeholder @{key}@ in {project} {tag}")

    generated = re.sub(r"@([A-Z0-9_]+)@", repl, template)

    if project == "libxml2":
        outname = "xmlversion.h"
    elif project == "libexslt":
        outname = "exsltconfig.h"
    else:
        outname = "xsltconfig.h"
    outdir = Path(outdir)
    outdir.mkdir(parents=True, exist_ok=True)
    outp = outdir / outname
    outp.write_text(generated)

    # Also write the profile record (provenance for the generated header)
    meta = {
        "project": project,
        "version": version,
        "git_tag": tag,
        "profile": profile,
        "generated_file": outname,
        "source_template": TEMPLATES[project],
        "options": with_options,
        "special": {k: str(v) for k, v in special.items()},
        "generator": "tools/archaeology/profileconfig.py",
    }
    (outdir / f"{outname}.profile.json").write_text(
        json.dumps(meta, indent=2, sort_keys=True))
    return outp, meta


def main():
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(1)
    project = sys.argv[1]
    version = sys.argv[2]
    tag = sys.argv[3]
    profile = "distro"
    outdir = ROOT / "atlas" / "config" / project / version
    args = sys.argv[4:]
    i = 0
    while i < len(args):
        if args[i] == "--profile" and i + 1 < len(args):
            profile = args[i + 1]; i += 2
        elif args[i] == "--out" and i + 1 < len(args):
            outdir = Path(args[i + 1]); i += 2
        else:
            i += 1
    if profile not in PROFILES:
        raise SystemExit(f"unknown profile {profile}; known: {sorted(PROFILES)}")
    outp, meta = generate(project, version, tag, profile, outdir)
    print(f"generated {outp} (profile={profile})")
    print(f"  {len(meta['options'])} feature options, source {meta['source_template']}")


if __name__ == "__main__":
    main()
