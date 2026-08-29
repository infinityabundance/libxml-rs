#!/usr/bin/env python3
"""Historical oracle manifest emitter (11.1-A).

Every built historical oracle gets an `oracle-manifest.json` beside its prefix
binding: upstream tag + resolved commit SHA + source-tree hash + adaptation
script hash + host/kernel/arch/libc + compiler/autoconf/automake/libtool +
exact configure argv + CPPFLAGS/CFLAGS/LDFLAGS + feature manifest + generated
config-header hash + built binary/library hashes + installed-header tree hash.

An oracle is identified by more than its `--version` string (§42, 11.1-A).
The matrix receipt hashes these manifests.

Usage:
  oracle_manifest.py argv <project> [libxml2-prefix]   # canonical configure argv (build.sh consumes this)
  oracle_manifest.py emit <project> <version>          # write oracle-manifest.json into the prefix
  oracle_manifest.py backfill                          # emit manifests for every existing prefix
"""
import glob
import hashlib
import json
import os
import platform
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "oracle", "historical")
ADAPT = os.path.join(OUT, "adapt", "autotools-modernize.sed")
DOX = os.path.join(OUT, "doxygen")
GIT = {"libxml2": os.path.join(ROOT, "archaeology", "libxml2-git"),
       "libxslt": os.path.join(ROOT, "archaeology", "libxslt-git")}

BASE_CONFIGURE_ARGS = [
    "--disable-shared", "--enable-static",
    "--without-python", "--without-http", "--without-ftp",
    "--without-icu", "--without-threads",
]

# 2.6.32 could not be built: configure.in of that era is not bridgeable to
# autoconf 2.73 even with autotools-modernize.sed (documented archaeology
# failure, per run_matrix.sh "2.6.32 excluded: era toolchain required").
DOCUMENTED_BUILD_FAILURES = {
    "libxml2-2.6.32": (
        "era toolchain required: configure.in of this era uses macros removed "
        "from modern autoconf/automake; autotools-modernize.sed cannot bridge "
        "the gap under autoconf 2.73. Excluded from the matrix (run_matrix.sh "
        "XML2_VERSIONS). Source identity below is still authoritative."
    ),
}


def resolve_tag(project, version):
    git = GIT[project]
    for cand in (version, f"v{version}",
                 f"LIBXML2.{version[2:]}",
                 "LIBXML_" + version.replace(".", "_")):
        r = subprocess.run(["git", "rev-parse", "-q", "--verify", f"refs/tags/{cand}"],
                           cwd=git, capture_output=True, text=True)
        if r.returncode == 0:
            return cand
    return None


def commit_sha(project, tag):
    git = GIT[project]
    r = subprocess.run(["git", "rev-parse", "refs/tags/{0}^{{}}".format(tag)],
                       cwd=git, capture_output=True, text=True)
    if r.returncode == 0 and r.stdout.strip():
        return r.stdout.strip()
    r = subprocess.run(["git", "rev-parse", f"refs/tags/{tag}"],
                       cwd=git, capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else None


def tree_hash(path):
    """Deterministic sha256 over (relative_path, content) of every file, sorted."""
    if not os.path.isdir(path):
        return None
    h = hashlib.sha256()
    for root, _dirs, files in sorted(os.walk(path)):
        for fn in sorted(files):
            full = os.path.join(root, fn)
            rel = os.path.relpath(full, path)
            try:
                with open(full, "rb") as f:
                    data = f.read()
            except OSError:
                continue
            h.update(rel.encode())
            h.update(b"\0")
            h.update(data)
            h.update(b"\0")
    return h.hexdigest()


def sha_file(path):
    try:
        with open(path, "rb") as f:
            return hashlib.sha256(f.read()).hexdigest()
    except OSError:
        return None


def host_identity():
    out = {}
    out["hostname"] = platform.node()
    out["kernel"] = platform.release()
    out["architecture"] = platform.machine()
    out["os"] = platform.system()
    ldd = subprocess.run(["ldd", "--version"], capture_output=True, text=True)
    out["libc"] = (ldd.stdout or ldd.stderr).splitlines()[0] if ldd.returncode == 0 else None
    gcc = subprocess.run(["gcc", "--version"], capture_output=True, text=True)
    out["compiler"] = gcc.stdout.splitlines()[0] if gcc.returncode == 0 else None
    for name, cmd in (("autoconf", ["/usr/bin/autoconf", "--version"]),
                      ("automake", ["/usr/bin/automake", "--version"]),
                      ("libtool", ["/usr/bin/libtoolize", "--version"])):
        r = subprocess.run(cmd, capture_output=True, text=True)
        out[name] = (r.stdout or r.stderr).splitlines()[0] if r.returncode == 0 else None
    return out


def configure_argv(project, prefix, libxml2_prefix=None):
    argv = [f"--prefix={prefix}"] + list(BASE_CONFIGURE_ARGS)
    if project == "libxslt":
        if not libxml2_prefix:
            raise SystemExit("libxslt requires the libxml2 prefix (--with-libxml-prefix)")
        argv.append(f"--with-libxml-prefix={libxml2_prefix}")
    return argv


def compiled_with(bin_path):
    if not os.path.exists(bin_path):
        return None
    r = subprocess.run([bin_path, "--version"], capture_output=True)
    txt = (r.stdout + r.stderr).decode(errors="replace")
    for line in txt.splitlines():
        if "compiled with:" in line:
            return line.strip()
    return None


def enabled_features(compiled_line):
    """Parse the `compiled with:` feature list from the built tool itself.

    The feature list varies by era (e.g. 2.6.x lacks Modules; 2.13.x drops
    DebugLegacy), so the parsed list is per-oracle evidence, not a constant.
    Returns None when the tool does not print a feature list (libxslt never
    does in the matrix span).
    """
    if not compiled_line:
        return None
    _, _, rest = compiled_line.partition("compiled with:")
    toks = rest.strip().split()
    return toks or None


def doxygen_profile(project, version):
    """Bind the 11.1-B Doxygen extraction identity for this oracle.

    The Doxygen run for a built version consumes the *installed headers of
    that same oracle* (profile input = prefix include dir), so the extraction
    identity is part of the oracle identity: Doxygen version, extraction
    config hash, extraction source-tree hash, raw XML hash, normalized
    inventory hash and the include path used.

    Returns None when no extraction exists (e.g. versions whose build
    failed and were therefore never run through Doxygen).
    """
    pdir = os.path.join(DOX, f"{project}-{version}")
    prof = os.path.join(pdir, "profile-public.json")
    inv = os.path.join(pdir, "inventory-public.json")
    if not (os.path.exists(prof) and os.path.exists(inv)):
        return None
    p = json.load(open(prof))
    i = json.load(open(inv))
    return {
        "profile": p.get("profile"),
        "doxygen_version": p.get("doxygen_version"),
        "config_hash": p.get("config_hash"),
        "extraction_source_tree_hash": p.get("source_tree_hash"),
        "include_path": os.path.relpath(p.get("input", ""), ROOT),
        "raw_xml_hash": i.get("raw_xml_hash"),
        "inventory_hash": i.get("inventory_hash"),
        "inventory_path": os.path.relpath(inv, ROOT),
        "entities": (i.get("counts") or {}).get("total"),
    }


def build_env_capture(prefix):
    """Exact configure-time environment, captured by build.sh as
    build-env.json beside the prefix (CPPFLAGS/CFLAGS/LDFLAGS/CC/PATH).
    Falls back to the documented values used by build.sh for oracles
    built before the capture mechanism existed."""
    path = os.path.join(prefix, "build-env.json")
    if os.path.exists(path):
        return json.load(open(path))
    return {
        "CPPFLAGS": "",
        "CFLAGS": "-O2 -w",
        "LDFLAGS": "",
        "CC": "",
        "note": "pre-capture build; documented values from build.sh (CFLAGS='-O2 -w', CPPFLAGS/LDFLAGS unset)",
    }


def emit(project, version, libxml2_prefix=None):
    prefix = os.path.join(OUT, "prefix", f"{project}-{version}")
    src = os.path.join(OUT, "src", f"{project}-{version}")
    tag = resolve_tag(project, version)
    commit = commit_sha(project, tag) if tag else None
    host = host_identity()
    adapt_hash = sha_file(ADAPT)

    cfg = configure_argv(project, prefix, libxml2_prefix)

    # feature manifest from the built tool itself where available
    bins = sorted(glob.glob(os.path.join(prefix, "bin", "*")))
    compiled = None
    for b in bins:
        base = os.path.basename(b)
        if project == "libxml2" and base == "xmllint":
            compiled = compiled_with(b)
        if project == "libxslt" and base == "xsltproc":
            compiled = compiled_with(b)

    # generated config header
    if project == "libxml2":
        cfg_header = os.path.join(prefix, "include", "libxml2", "libxml", "xmlversion.h")
    else:
        cfg_header = os.path.join(prefix, "include", "libxslt", "xsltconfig.h")
    if not os.path.exists(cfg_header):
        cfg_header = None

    # build status: a built oracle has artifacts; a failed build is a
    # documented archaeology failure residual, never silent nulls
    built = bool(glob.glob(os.path.join(prefix, "lib", "*.a"))) or bool(bins)
    key = f"{project}-{version}"
    if built:
        build_status = "BUILT"
        build_failure_reason = None
    else:
        build_status = "FAILED"
        build_failure_reason = DOCUMENTED_BUILD_FAILURES.get(key)

    benv = build_env_capture(prefix)

    manifest = {
        "schema": "oracle-manifest-2",
        "project": project,
        "version": version,
        "build_status": build_status,
        "build_failure_reason": build_failure_reason,
        "resolved_git_tag": tag,
        "upstream_commit_sha": commit,
        "source_tree_hash": tree_hash(src),
        "source_tree_path": os.path.relpath(src, ROOT),
        "adaptation_script": "oracle/historical/adapt/autotools-modernize.sed",
        "adaptation_script_hash": adapt_hash,
        "host": {
            "hostname": host["hostname"],
            "os": host["os"],
            "kernel": host["kernel"],
            "architecture": host["architecture"],
            "libc": host["libc"],
        },
        "build_environment": {
            "compiler": host["compiler"],
            "autoconf": host["autoconf"],
            "automake": host["automake"],
            "libtool": host["libtool"],
            "path": "/usr/bin:$PATH (cargo bin shadows autotools with Rust shims; forced to system)",
        },
        "configuration": {
            "configure_argv": cfg,
            "CPPFLAGS": benv.get("CPPFLAGS", ""),
            "CFLAGS": benv.get("CFLAGS", ""),
            "LDFLAGS": benv.get("LDFLAGS", ""),
            "CC": benv.get("CC", ""),
            "enabled_features": enabled_features(compiled) if project == "libxml2" else None,
            "disabled_features": ["python", "http", "ftp", "icu", "threads", "shared"],
            "runtime_compiled_with": compiled,
            "build_env_capture_note": benv.get("note"),
        },
        "generated_config_header_hash": sha_file(cfg_header) if cfg_header else None,
        "built_binary_hashes": {os.path.basename(b): sha_file(b) for b in bins},
        "built_library_hashes": {
            os.path.relpath(f, os.path.join(prefix, "lib")): sha_file(f)
            for f in sorted(glob.glob(os.path.join(prefix, "lib", "*.a")))
        },
        "installed_header_tree_hash": tree_hash(os.path.join(prefix, "include")),
        "doxygen_profile": doxygen_profile(project, version),
    }
    out_path = os.path.join(prefix, "oracle-manifest.json")
    with open(out_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write("\n")
    # Committed evidence copy: deleting build prefixes must not destroy oracle
    # identity (11.1-A). The matrix receipt hashes these committed manifests.
    evidence_dir = os.path.join(ROOT, "atlas", "oracle-manifests")
    os.makedirs(evidence_dir, exist_ok=True)
    ev_path = os.path.join(evidence_dir, f"{project}-{version}.json")
    with open(ev_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print("manifest:", out_path)
    print("evidence copy:", ev_path)
    return out_path


def backfill():
    pairs = {"1.1.26": "2.7.8", "1.1.32": "2.9.4", "1.1.35": "2.9.10",
             "1.1.38": "2.10.4", "1.1.42": "2.11.5"}
    for project in ("libxml2", "libxslt"):
        pdir = os.path.join(OUT, "prefix", f"{project}-*")
        for prefix in sorted(glob.glob(pdir)):
            version = os.path.basename(prefix)[len(project) + 1:]
            if not os.path.isdir(prefix):
                continue
            libxml2_prefix = None
            if project == "libxslt":
                xmlv = pairs.get(version)
                libxml2_prefix = os.path.join(OUT, "prefix", f"libxml2-{xmlv}") if xmlv else None
            emit(project, version, libxml2_prefix)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else None
    if cmd == "argv":
        proj = sys.argv[2]
        prefix = sys.argv[3]
        libp = sys.argv[4] if len(sys.argv) > 4 else None
        print(" ".join(configure_argv(proj, prefix, libp)))
    elif cmd == "emit":
        emit(sys.argv[2], sys.argv[3],
             sys.argv[4] if len(sys.argv) > 4 else None)
    elif cmd == "backfill":
        backfill()
    else:
        print(__doc__)
        sys.exit(1)
