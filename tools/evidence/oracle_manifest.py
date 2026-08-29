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
GIT = {"libxml2": os.path.join(ROOT, "archaeology", "libxml2-git"),
       "libxslt": os.path.join(ROOT, "archaeology", "libxslt-git")}

BASE_CONFIGURE_ARGS = [
    "--disable-shared", "--enable-static",
    "--without-python", "--without-http", "--without-ftp",
    "--without-icu", "--without-threads",
]


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


def emit(project, version, libxml2_prefix=None):
    prefix = os.path.join(OUT, "prefix", f"{project}-{version}")
    src = os.path.join(OUT, "src", f"{project}-{version}")
    tag = resolve_tag(project, version)
    commit = commit_sha(project, tag) if tag else None
    host = host_identity()
    adapt_hash = sha_file(ADAPT)

    cfg = configure_argv(project, prefix, libxml2_prefix)
    cflags = "-O2 -w"

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

    manifest = {
        "schema": "oracle-manifest-1",
        "project": project,
        "version": version,
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
            "CPPFLAGS": "",
            "CFLAGS": cflags,
            "LDFLAGS": "",
            "enabled_features": None,
            "disabled_features": ["python", "http", "ftp", "icu", "threads", "shared"],
            "runtime_compiled_with": compiled,
        },
        "generated_config_header_hash": sha_file(cfg_header) if cfg_header else None,
        "built_binary_hashes": {os.path.basename(b): sha_file(b) for b in bins},
        "built_library_hashes": {
            os.path.relpath(f, os.path.join(prefix, "lib")): sha_file(f)
            for f in sorted(glob.glob(os.path.join(prefix, "lib", "*.a")))
        },
        "installed_header_tree_hash": tree_hash(os.path.join(prefix, "include")),
        "doxygen_profile": None,  # filled by 11.1-B
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
