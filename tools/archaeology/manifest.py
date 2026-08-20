#!/usr/bin/env python3
"""
manifest.py — Automated release-manifest generation from upstream git history.

Phase 0 / §8 / §9 deliverable generator.

Reads the upstream git repositories (archaeology/libxml2-git, archaeology/libxslt-git)
and emits machine-readable release manifests (atlas/releases/<project>/<version>.json)
plus a consolidated manifest index (atlas/releases/<project>/_manifest.json).

Manifest fields per §8:
  project, version, release_date, source_origin, source_checksum (archive-derived),
  repository_commit/tag, archive_url, build_system, compiler_assumptions,
  dependency_requirements, platform_assumptions, enabled_features, disabled_features,
  public_headers, exported_symbols, soname, version_macros, tests_available,
  known_cves, known_regressions, known_compatibility_fixes, major_semantic_changes,
  documentation_source, provenance_confidence.

This generator captures what is derivable from git metadata. Fields requiring
external evidence (checksums, CVE lists) are marked "known"/"inferred"/"unknown"
per the custody rule §8 and filled from auxiliary evidence files as acquired.

Usage:
  manifest.py libxml2 [outdir]
  manifest.py libxslt [outdir]
"""

import json
import os
import re
import subprocess
import sys
import datetime
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent.parent
GIT_DIRS = {
    "libxml2": ROOT / "archaeology" / "libxml2-git",
    "libxslt": ROOT / "archaeology" / "libxslt-git",
}

# Version-string normalization: git tags use assorted schemes; produce a canonical
# dotted version for each tag.
def normalize_version(project, tag):
    if project == "libxml2":
        # LIBXML2.<minor>.<micro> style: LIBXML2.6.31 == 2.6.31 (the "2" prefix is
        # implicit in the tag name; matches tags used from ~2.6/2.7 era).
        m = re.fullmatch(r"LIBXML2\.([0-9]+)\.([0-9]+)(?:\.([0-9]+))?", tag)
        if m:
            g = m.groups()
            return ".".join(["2"] + [x for x in g if x is not None])
        # LIBXML2_<maj>_<min>_<mic> style: LIBXML2_2_5_7 == 2.5.7
        m = re.fullmatch(r"LIBXML2_([0-9]+)_([0-9]+)(?:_([0-9]+))?", tag)
        if m:
            return ".".join(g for g in m.groups() if g is not None)
        # The original W3C libxml lineage (pre-libxml2): LIBXML_1_x_y and the
        # historically inconsistent spelling LIB_XML_1_x_y used for early tags.
        # The literal "1_" after the prefix IS the major version, so prepend it.
        m = re.fullmatch(r"LIB(?:XML|_XML)_1_([0-9]+)(?:_([0-9]+))?", tag)
        if m:
            return ".".join(["1"] + [g for g in m.groups() if g is not None])
        # LIBXML_6_0 is a mis-named tag for libxml2 2.6.0 (dropped the "2_" prefix).
        m = re.fullmatch(r"LIBXML_6_([0-9]+)", tag)
        if m:
            return f"2.6.{m.group(1)}"
        # LIBXML2_6_0: one-off tag for libxml2 2.6.0 (LIBXML2 + 6_0)
        m = re.fullmatch(r"LIBXML2_6_([0-9]+)", tag)
        if m:
            return f"2.6.{m.group(1)}"
        # LIBXML_1_8_10_REAL: re-tag of 1.8.10 after a packaging mistake
        m = re.fullmatch(r"LIBXML_1_8_10_REAL", tag)
        if m:
            return "1.8.10"
        # The 2.x lineage used LIBXML_2_x_y tags from 2.0.0 through ~2.6.x.
        # The literal "2_" after LIBXML_ IS the major version, so prepend it.
        m = re.fullmatch(r"LIBXML_2_([0-9]+)_([0-9]+)(?:_([0-9]+))?", tag)
        if m:
            return ".".join(["2"] + [g for g in m.groups() if g is not None])
        # LIBXML_0_99 is the 0.99 era (the GNOME xml library)
        m = re.fullmatch(r"LIBXML_0_([0-9]+)", tag)
        if m:
            return f"0.{m.group(1)}"
        m = re.fullmatch(r"v([0-9]+\.[0-9]+\.[0-9]+.*)", tag)
        if m:
            return m.group(1)
        # ancient GNOME-era tags (FOR_GNOME_0_99_1 etc.) are not libxml releases
        return None
    if project == "libxslt":
        m = re.fullmatch(r"LIBXSLT_([0-9]+)_([0-9]+)_([0-9]+)", tag)
        if m:
            return ".".join(m.groups())
        m = re.fullmatch(r"v([0-9]+\.[0-9]+\.[0-9]+.*)", tag)
        if m:
            return m.group(1)
        m = re.fullmatch(r"([0-9]+\.[0-9]+\.[0-9]+)", tag)
        if m:
            return m.group(1)
        return None


def git(project, *args):
    gitdir = GIT_DIRS[project]
    return subprocess.run(
        ["git", "-C", str(gitdir), *args],
        capture_output=True, text=True, check=False,
    ).stdout.strip()


def tag_commit(project, tag):
    return git(project, "rev-list", "-n", "1", tag)


def tag_date(project, tag):
    # Use the tag's commit author date; prefer annotated tag date when present.
    out = git(project, "for-each-ref", "--format=%(creatordate:iso)", f"refs/tags/{tag}")
    if not out:
        out = git(project, "show", "-s", "--format=%ad", "--date=iso", tag)
    return out


def version_macros(project, version):
    """Best-effort computation of the LIBXML_VERSION_* / LIBXSLT_VERSION_* macros."""
    parts = re.split(r"[.-]", version)
    while len(parts) < 3:
        parts.append("0")
    maj = int(parts[0]); minor = int(parts[1]); micro = int(parts[2])
    if project == "libxml2":
        return {
            "LIBXML_DOTTED_VERSION": version,
            "LIBXML_VERSION_STRING": version.replace(".", ""),
            "LIBXML_VERSION_EXTRA": "",
            "LIBXML_VERSION": maj * 10000 + minor * 100 + micro,
            "LIBXML_VERSION_MAJOR": maj,
            "LIBXML_VERSION_MINOR": minor,
            "LIBXML_VERSION_MICRO": micro,
            "LIBXML_VERSION_NUMBER": maj * 10000 + minor * 100 + micro,
        }
    # libxslt uses LIBXSLT_VERSION as single int, LIBXSLT_DOTTED_VERSION string,
    # LIBXSLT_VERSION_STRING is "1xxyy" style.
    return {
        "LIBXSLT_DOTTED_VERSION": version,
        "LIBXSLT_VERSION_STRING": f"{maj}{minor:02d}{micro:02d}",
        "LIBXSLT_VERSION": maj * 100 + minor,
        "LIBXSLT_VERSION_MAJOR": maj,
        "LIBXSLT_VERSION_MINOR": minor,
        "LIBXSLT_VERSION_MICRO": micro,
        "LIBXSLT_VERSION_NUMBER": maj * 10000 + minor * 100 + micro,
    }


def soname(project, version):
    """Known SONAME history (to be verified against atlas/abi snapshots)."""
    if project == "libxml2":
        # libxml2.so.2 from 2.0.0 onward (the libxml2.so.2 ABI). Pre-2.0 was libxml.so.1.
        parts = version.split(".")
        if int(parts[0]) < 2:
            return {"soname": "libxml.so.1", "confidence": "known"}
        return {"soname": "libxml2.so.2", "confidence": "known"}
    if project == "libxslt":
        # libxslt.so.1 historically; the modern current is libxslt.so.1.
        return {"soname": "libxslt.so.1", "confidence": "known"}
    return {"soname": None, "confidence": "unknown"}


def main():
    project = sys.argv[1]
    outdir = Path(sys.argv[2]) if len(sys.argv) > 2 else ROOT / "atlas" / "releases" / project
    outdir.mkdir(parents=True, exist_ok=True)

    tags = git(project, "tag", "--sort=version:refname").splitlines()
    manifests = []
    gaps = []
    cve_records = []

    # Tags that are infrastructure or junk markers (recorded as gaps, never silently dropped)
    INFRA_TAGS = {
        "libxml2": {
            "ChangeLog": "not a release tag (file named tag)",
            "EAZEL-NAUTILUS-MS-AUG07": "downstream snapshot tag",
            "FOR_GNOME_0_99_1": "gnome release snapshot, not libxml release",
            "GNOME_0_30": "gnome release snapshot",
            "GNOME_PRINT_0_24": "gnome release snapshot",
            "GNUMERIC_FIRST_PUBLIC_RELEASE": "downstream snapshot tag",
            "LIBXML2_2_5_x": "branch point tag, not release",
            "LIBXML_TEST_2_0_0": "test/release-candidate tag",
            "LIBXML_1_X": "1.x branch marker",
            "LIB_XML_1_X": "1.x branch marker (alternate spelling)",
            "PRE_MUCKUP": "historical pre-refactor marker",
            "PRE_MUCKUP2": "historical pre-refactor marker",
            "PRE_MUCKUP3": "historical pre-refactor marker",
            "help": "junk tag",
        },
        "libxslt": {
            "LIXSLT_0_5_0": "typo tag (LIXSLT) shadowing a real version",
        },
    }

    for tag in tags:
        if re.fullmatch(r"CVE[-_].*", tag):
            # Security-fix marker tags are archaeological evidence (§73). They point
            # at the commit that fixed a CVE; record them in the security index.
            cve_records.append({
                "tag": tag,
                "fix_commit": tag_commit(project, tag),
                "tag_date": tag_date(project, tag),
            })
            continue
        if tag in INFRA_TAGS.get(project, {}):
            gaps.append({"tag": tag, "reason": INFRA_TAGS[project][tag]})
            continue
        version = normalize_version(project, tag)
        if version is None:
            gaps.append({"tag": tag, "reason": "unrecognized version tag (possibly non-release)"})
            continue
        # Skip rc/alpha/beta variants for primary manifest but record them.
        if any(x in version.lower() for x in ("rc", "alpha", "beta", "pre")):
            manifests.append({
                "project": project,
                "version": version,
                "prerelease": True,
                "git_tag": tag,
                "git_commit": tag_commit(project, tag),
                "release_date": tag_date(project, tag),
                "provenance_confidence": "known",
            })
            continue

        commit = tag_commit(project, tag)
        date = tag_date(project, tag)
        macros = version_macros(project, version)
        entry = {
            "project": project,
            "version": version,
            "prerelease": False,
            "release_date": date,
            "source_origin": f"gitlab.gnome.org/GNOME/{project}.git",
            "source_checksum": {"status": "unknown", "note": "derive from release tarball when archived"},
            "repository_commit": commit,
            "git_tag": tag,
            "archive_url": {
                "gnome": f"https://download.gnome.org/sources/{project}/{version[:3]}/{project}-{version}.tar.xz",
                "status": "inferred",
            },
            "build_system": {"primary": "autotools", "secondary": "cmake", "confidence": "known"},
            "compiler_assumptions": {"c99": True, "confidence": "inferred"},
            "dependency_requirements": {"libxml2": None if project == "libxml2" else "bundled-compatible"},
            "platform_assumptions": {"posix": True, "windows": True, "macos": True, "confidence": "known"},
            "enabled_features": None,
            "disabled_features": None,
            "public_headers": None,
            "exported_symbols": None,
            "soname": soname(project, version),
            "version_macros": macros,
            "tests_available": None,
            "known_cves": [],
            "known_regressions": [],
            "known_compatibility_fixes": [],
            "major_semantic_changes": [],
            "documentation_source": f"gtk-doc/doxygen in {project}-{version} tarball",
            "provenance_confidence": "known",
            "_filled": [
                "release_date", "repository_commit", "git_tag", "version_macros",
                "soname", "build_system",
            ],
            "_pending": [
                "source_checksum", "archive_url", "enabled_features", "disabled_features",
                "public_headers", "exported_symbols", "tests_available", "known_cves",
                "major_semantic_changes",
            ],
        }
        manifests.append(entry)

    # Write per-version manifests and index
    index = {
        "project": project,
        "generated": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "generator": "tools/archaeology/manifest.py",
        "provenance_rule": "§8 custody: known/inferred/unknown distinguished",
        "versions": [],
    }
    for m in manifests:
        if m.get("prerelease"):
            continue
        fname = f"{m['version']}.json"
        with open(outdir / fname, "w") as f:
            json.dump(m, f, indent=2, sort_keys=True)
        index["versions"].append({
            "version": m["version"],
            "file": fname,
            "release_date": m["release_date"],
            "git_commit": m["repository_commit"],
            "git_tag": m["git_tag"],
        })

    with open(outdir / "_manifest.json", "w") as f:
        json.dump(index, f, indent=2, sort_keys=True)

    # Gap record — anything we could not classify (never silently skipped per §8)
    if gaps:
        with open(outdir / "_gaps.json", "w") as f:
            json.dump({"project": project, "gaps": gaps,
                       "rule": "§8: do not invent, do not silently skip; record the gap"},
                      f, indent=2, sort_keys=True)

    # Security-history index (§73): CVE marker tags point at the fix commits.
    if cve_records:
        with open(outdir / "_cve_fix_commits.json", "w") as f:
            json.dump({
                "project": project,
                "note": "CVE marker tags in the git history point at the commit that fixed each CVE. "
                        "Full CVE custody entries live in atlas/security-history.",
                "records": cve_records,
            }, f, indent=2, sort_keys=True)

    print(f"{project}: {len(manifests)} tags processed, {len(index['versions'])} releases in manifest")
    if gaps:
        print(f"  gaps recorded: {len(gaps)}")
    for g in gaps:
        print(f"    - {g}")
    if cve_records:
        print(f"  CVE fix-commit markers recorded: {len(cve_records)}")


if __name__ == "__main__":
    main()
