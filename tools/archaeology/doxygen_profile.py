#!/usr/bin/env python3
"""Doxygen extraction profiles for the forensic surface census (11.1-B/11.1-C).

A profile pins: pristine upstream source (extracted fresh from the archaeology
git clone — never the sed-modernized build trees), the real generated config
header of the matching oracle build, the aggressive forensic Doxygen options,
and the identity hashes (profile/config/source/doxygen-version).

Profiles:
  public — INPUT = installed public headers of the built oracle (the consumer
           ABI/API surface, with the era's real generated config header).
  full   — INPUT = the complete pristine source tree (.c + .h); internal and
           static surfaces included (11.1-C: internal entities that influence
           observable behavior are relevant).

Usage:
  doxygen_profile.py gen <project> <version> [public|full]
  doxygen_profile.py gen system [public|full]
"""
import hashlib
import json
import os
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "oracle", "historical")
DOX = os.path.join(OUT, "doxygen")
GIT = {"libxml2": os.path.join(ROOT, "archaeology", "libxml2-git"),
       "libxslt": os.path.join(ROOT, "archaeology", "libxslt-git"),
       # libexslt ships inside the libxslt tree (libexslt/); its own version
       # (0.8.x) has no separate git tags, so historical extractions resolve
       # through the libxslt tag that shipped the matching exslt release.
       "libexslt": os.path.join(ROOT, "archaeology", "libxslt-git")}
# libexslt version -> libxslt git tag that shipped it (system 0.8.25 == 1.1.45)
EXSLT_TAG_MAP = {"0.8.25": "v1.1.45"}
PREFIX = os.path.join(OUT, "prefix")

AGGRESSIVE = {
    "GENERATE_XML": "YES",
    "XML_PROGRAMLISTING": "YES",
    "EXTRACT_ALL": "YES",
    "EXTRACT_PRIVATE": "YES",
    "EXTRACT_PRIV_VIRTUAL": "YES",
    "EXTRACT_STATIC": "YES",
    "EXTRACT_LOCAL_CLASSES": "YES",
    "EXTRACT_LOCAL_METHODS": "YES",
    "JAVADOC_AUTOBRIEF": "YES",
    "HIDE_UNDOC_MEMBERS": "NO",
    "HIDE_UNDOC_CLASSES": "NO",
    "ENABLE_PREPROCESSING": "YES",
    "MACRO_EXPANSION": "YES",
    "EXPAND_ONLY_PREDEF": "NO",
    "SEARCH_INCLUDES": "YES",
    "SKIP_FUNCTION_MACROS": "NO",
    "REFERENCES_RELATION": "YES",
    "REFERENCED_BY_RELATION": "YES",
    "SOURCE_BROWSER": "YES",
    "GENERATE_HTML": "NO",
    "GENERATE_LATEX": "NO",
    "GENERATE_RTF": "NO",
    "GENERATE_MAN": "NO",
    "GENERATE_DOCBOOK": "NO",
    "QUIET": "YES",
    "WARNINGS": "YES",
    "WARN_IF_UNDOCUMENTED": "NO",
    "WARN_IF_DOC_ERROR": "YES",
    "RECURSIVE": "YES",
    "SORT_MEMBER_DOCS": "NO",
    "SORT_BRIEF_DOCS": "NO",
    "SORT_MEMBERS_CTORS_1ST": "NO",
    "SORT_GROUP_NAMES": "NO",
    "SORT_BY_SCOPE_NAME": "NO",
    "SHOW_FILES": "YES",
    "SHOW_HEADERFILE": "YES",
    "PREDEFINED": "",
}


def resolve_tag(project, version):
    git = GIT[project]
    if project == "libexslt" and version in EXSLT_TAG_MAP:
        return EXSLT_TAG_MAP[version]
    for cand in (version, f"v{version}",
                 f"LIBXML2.{version[2:]}",
                 "LIBXML_" + version.replace(".", "_")):
        r = subprocess.run(["git", "rev-parse", "-q", "--verify", f"refs/tags/{cand}"],
                           cwd=git, capture_output=True, text=True)
        if r.returncode == 0:
            return cand
    return None


def extract_pristine(project, version, tag):
    """Fresh extraction of the upstream tree — never the adapted build tree."""
    work = os.path.join(DOX, f"{project}-{version}", "src")
    marker = os.path.join(work, ".pristine-ok")
    git = GIT[project]
    r = subprocess.run(["git", "rev-parse", f"refs/tags/{tag}^{{}}"],
                       cwd=git, capture_output=True, text=True)
    commit = r.stdout.strip() if r.returncode == 0 else None
    if os.path.exists(marker):
        with open(marker) as f:
            if f.read().strip() == (commit or tag):
                return work, commit
    os.makedirs(work, exist_ok=True)
    subprocess.run(["git", "archive", tag], cwd=git, check=True,
                   stdout=open(os.path.join(DOX, f"{project}-{version}", "archive.tar"), "wb"))
    subprocess.run(["tar", "-x", "-C", work, "-f",
                    os.path.join(DOX, f"{project}-{version}", "archive.tar")], check=True)
    with open(marker, "w") as f:
        f.write(commit or tag)
    return work, commit


def tree_hash(path):
    h = hashlib.sha256()
    for root, _dirs, files in sorted(os.walk(path)):
        for fn in sorted(files):
            full = os.path.join(root, fn)
            rel = os.path.relpath(full, path)
            try:
                data = open(full, "rb").read()
            except OSError:
                continue
            h.update(rel.encode())
            h.update(b"\0")
            h.update(data)
            h.update(b"\0")
    return h.hexdigest()


def doxygen_version():
    r = subprocess.run(["doxygen", "--version"], capture_output=True, text=True)
    return r.stdout.strip() or r.stderr.strip()


def gen(project, version, profile="public"):
    tag = resolve_tag(project, version) if version != "system" else None
    work = os.path.join(DOX, f"{project}-{version}")
    if version == "system":
        # system oracle: use the installed system headers
        src = {"libxml2": "/usr/include/libxml2",
               "libxslt": "/usr/include/libxslt",
               "libexslt": "/usr/include/libexslt"}[project]
        commit = None
        src_hash = None
        prefix_inc = src
    else:
        if tag is None:
            print(f"no tag for {project} {version}")
            return 1
        src, commit = extract_pristine(project, version, tag)
        src_hash = tree_hash(src)
        prefix_inc = os.path.join(PREFIX, f"{project}-{version}", "include")
        if project == "libxml2":
            prefix_inc = os.path.join(prefix_inc, "libxml2")
        if project == "libexslt" and not os.path.isdir(prefix_inc):
            # No prefix capture exists for libexslt. Build a headers-only
            # input matching the installed consumer surface (exslt.h +
            # exsltexports.h + the generated exsltconfig.h), so the
            # historical profile is comparable with the system profile.
            prefix_inc = os.path.join(work, "pubhdr")
            os.makedirs(prefix_inc, exist_ok=True)
            for hdr in ("exslt.h", "exsltexports.h", "libexslt.h"):
                shutil.copy2(os.path.join(src, "libexslt", hdr),
                             os.path.join(prefix_inc, hdr))
            shutil.copy2(os.path.join("/usr/include/libexslt", "exsltconfig.h"),
                         os.path.join(prefix_inc, "exsltconfig.h"))

    if profile == "public":
        inputs = prefix_inc
    else:
        inputs = src

    work = os.path.join(DOX, f"{project}-{version}")
    xml_out = os.path.join(work, f"xml-{profile}")
    os.makedirs(xml_out, exist_ok=True)

    cfg = dict(AGGRESSIVE)
    cfg["PROJECT_NAME"] = f"{project} {version} ({profile})"
    cfg["OUTPUT_DIRECTORY"] = work
    cfg["XML_OUTPUT"] = f"xml-{profile}"
    cfg["INPUT"] = inputs
    cfg["INCLUDE_PATH"] = prefix_inc
    # era-appropriate preprocessor surface: the built oracle's feature macros
    # plus the tree.h circular-dependency workaround define (R-000005: without
    # XML_TREE_INTERNALS, tree.h hides all tree declarations)
    cfg["PREDEFINED"] = "LIBXML_STATIC XML_TREE_INTERNALS"

    doxyfile = os.path.join(work, f"Doxyfile-{profile}")
    with open(doxyfile, "w") as f:
        for k, v in cfg.items():
            f.write(f"{k} = {v}\n")
    cfg_hash = hashlib.sha256(open(doxyfile, "rb").read()).hexdigest()

    profile_doc = {
        "schema": "doxygen-profile-1",
        "profile": profile,
        "project": project,
        "version": version,
        "resolved_git_tag": tag,
        "upstream_commit_sha": commit,
        "source_tree": src,
        "source_tree_hash": src_hash,
        "input": inputs,
        "include_path": prefix_inc,
        "doxygen_version": doxygen_version(),
        "doxyfile": doxyfile,
        "config_hash": cfg_hash,
        "options": {k: v for k, v in cfg.items() if k in AGGRESSIVE},
    }
    with open(os.path.join(work, f"profile-{profile}.json"), "w") as f:
        json.dump(profile_doc, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"profile written: {os.path.join(work, f'profile-{profile}.json')} (config_hash {cfg_hash[:12]})")
    return 0


if __name__ == "__main__":
    if len(sys.argv) < 4:
        print(__doc__)
        sys.exit(1)
    sys.exit(gen(sys.argv[2], sys.argv[3], sys.argv[4] if len(sys.argv) > 4 else "public"))
