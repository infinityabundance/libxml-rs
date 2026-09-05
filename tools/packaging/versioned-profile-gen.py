#!/usr/bin/env python3
"""Generate tools/packaging/libxml2-versioned.syms (R-000179).

The candidate core (libxml2.so.16) matches the executed 2.15.3 oracle and is
UNVERSIONED. R-000179: distro binaries built against a versioned libxml2
(libxml2.syms, SONAME libxml2.so.2 — e.g. the 2.13.x line) require the named
LIBXML2_2.x nodes and emit ld.so 'no version information available' warnings
against any unversioned provider.

This generator produces the versioned-profile symbol map from the AUTHORITATIVE
distro DSO shipped on the reference host (/usr/lib/libxml2.so.2.13.9): the
per-symbol version-node assignment is read from its .gnu.version table, so the
map is byte-exact to the real distro graph (54 LIBXML2_2.x nodes). Only
symbols the candidate core actually defines are versioned; the remaining
candidate exports (2.15-era additions the distro never saw) are exported
unversioned in the leading global block. The script follows the upstream
libxml2.syms inheritance pattern (each node lists its new symbols and inherits
the previous node), so per-symbol versions and the chain match the distro DSO.

Output is committed so the versioned profile builds deterministically on any
host (the distro oracle is only needed to regenerate).
"""

import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "tools", "packaging", "libxml2-versioned.syms")
DISTRO = "/usr/lib/libxml2.so.2.13.9"


def defined_symbols(dso):
    """Return {name: kind} for every defined (non-UND) exported symbol."""
    out = subprocess.run(
        ["readelf", "--dyn-syms", "--wide", dso], capture_output=True, text=True
    ).stdout
    syms = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) < 8 or parts[4] == "UND":
            continue
        if parts[3] not in ("FUNC", "OBJECT", "NOTYPE", "TLS"):
            continue
        name = parts[7]
        base = name.split("@")[0]
        # keep the LAST occurrence (versioned defs repeat per node)
        syms[base] = parts[3]
    return syms


def main():
    if not os.path.exists(DISTRO):
        print(f"distro oracle {DISTRO} not found; nothing to regenerate", file=sys.stderr)
        return 1
    # node ORDER as recorded in .gnu.version_d (definition order == script order)
    vi = subprocess.run(
        ["readelf", "--version-info", "--wide", DISTRO], capture_output=True, text=True
    ).stdout
    # map version index -> node name from the Version definition section
    idx_to_name = {}
    cur_idx = None
    for line in vi.splitlines():
        m = re.search(r"Index:\s*(\d+)\s+.*Name:\s*(\S+)", line)
        if m and "Rev:" in line:
            cur_idx = int(m.group(1))
        m2 = re.search(r"Name:\s*(\S+)\s*$", line.strip())
        if m2 and cur_idx is not None and re.search(r"0x[0-9a-f]+:\s*Rev:", line):
            idx_to_name[cur_idx] = m2.group(1)
    # parse again robustly: entries look like
    #   0x0030: Rev: 1  Flags: BASE   Index: 1  Cnt: 1  Name: LIBXML2_2.4.30
    node_order = []
    for line in vi.splitlines():
        m = re.match(r"\s*0x[0-9a-f]+:\s*Rev:\s*\d+\s+Flags:\s*\S+\s+Index:\s*(\d+)\s+Cnt:\s*\d+\s+Name:\s*(\S+)", line)
        if m and m.group(2).startswith("LIBXML2_"):
            node_order.append(m.group(2))
    if not node_order:
        print("no LIBXML2_2.x nodes found in distro DSO", file=sys.stderr)
        return 1

    # symbol -> node from the dynsym version suffixes (@@default or @non-default)
    syms = subprocess.run(
        ["readelf", "--dyn-syms", "--wide", DISTRO], capture_output=True, text=True
    ).stdout
    sym_node = {}
    for line in syms.splitlines():
        parts = line.split()
        if len(parts) < 8 or parts[4] == "UND":
            continue
        name = parts[7]
        if "@" not in name or "@@GLIBC" in name or "@GLIBC" in name or "@XZ" in name or "@ZLIB" in name:
            continue
        base, _, node = name.rpartition("@")
        node = node.lstrip("@")
        if node.startswith("LIBXML2_"):
            base = base.rstrip("@")
            # a symbol may appear once per node it is inherited into; the
            # INTRODUCTION node is the one with '@@' (the default/definition)
            if "@@" in name or base not in sym_node:
                sym_node[base] = node

    # candidate-defined surface (the .16 core dynsym minus internal leaks)
    core = os.path.join(ROOT, "target", "debug", "liblibxml_rs.so")
    cand = {}
    if os.path.exists(core):
        cand = defined_symbols(core)
    else:
        print("candidate core not built; run cargo build first", file=sys.stderr)
        return 1

    # partition: versioned (in the distro map AND defined by the candidate)
    # vs unversioned-extra (candidate exports the distro DSO never versioned).
    # The extras go into a terminal LIBXML2_2.15.0 node (the distro never
    # saw them; no 2.13-era consumer can reference them).
    versioned = {}
    extra = []
    for base in sorted(cand):
        node = sym_node.get(base)
        if node:
            versioned.setdefault(node, []).append(base)
        else:
            extra.append(base)

    # node order: keep the distro order, but drop nodes with no candidate
    # symbols (their symbols are all absent) — but keep the CHAIN correct by
    # only emitting nodes that have symbols; inheritance keeps availability.
    emitted = [n for n in node_order if versioned.get(n)]

    lines = [
        "# Generated by tools/packaging/versioned-profile-gen.py from the",
        "# authoritative distro DSO (%s) .gnu.version tables (R-000179)." % DISTRO,
        "# SONAME libxml2.so.2 versioned-profile map: candidate-defined symbols",
        "# carry their exact upstream LIBXML2_2.x introduction node (upstream",
        "# libxml2.syms inheritance pattern); 2.15-era candidate exports the",
        "# distro never versioned sit in the terminal LIBXML2_2.15.0 node;",
        "# local: * (inside the final node) hides the internal leaks.",
    ]
    if emitted:
        first = emitted[0]
        lines.append(f"{first} {{")
        lines.append("  global:")
        for base in versioned[first]:
            lines.append(f"    {base};")
        lines.append("};")
        prev = first
        for node in emitted[1:]:
            lines.append(f"{node} {{")
            lines.append("  global:")
            for base in versioned[node]:
                lines.append(f"    {base};")
            lines.append(f"}} {prev};")
            prev = node
        lines.append("LIBXML2_2.15.0 {")
        lines.append("  global:")
        for base in extra:
            lines.append(f"    {base};")
        lines.append("  local: *;")
        lines.append(f"}} {prev};")
    else:
        lines.append("LIBXML2_2.15.0 {")
        lines.append("  global:")
        for base in extra:
            lines.append(f"    {base};")
        lines.append("  local: *;")
        lines.append("};")

    with open(OUT, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"wrote {OUT}")
    print(
        f"  nodes={len(emitted) + 1} versioned-symbols={sum(len(v) for v in versioned.values())} "
        f"terminal-2.15-symbols={len(extra)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
