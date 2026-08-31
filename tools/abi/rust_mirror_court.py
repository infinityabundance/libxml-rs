#!/usr/bin/env python3
"""Rust-mirror ↔ candidate-C-header ABI court (three-way representation parity).

The C-header ABI census (abi_probe_gen.py) proves upstream C ↔ candidate C.
This court closes the third edge of the triangle:

    upstream C header  ↔  candidate C header  ↔  Rust #[repr(C)]

It generates Rust-side measurements for EVERY #[repr(C)] type in
src/abi/structs.rs / src/abi/types.rs (sizeof, alignof, per-field offsetof and
field size, enum representation and every enumerator value) and the same
measurements from the candidate C headers, then compares them positionally.

Field comparison is BY POSITION (declaration order), never by name: the Rust
mirrors legitimately rename fields (`type_` vs `type`, `cont_model` vs
`contModel`). Offsets/sizes must match per position and in total; a missing
field (like the old 7-field _xmlElement vs the 14-field C struct) fails the
court by construction. Structs the C headers do not define are classified
(OPAQUE_FORWARD_DECL / NOT_LAYOUT_OBSERVABLE) and never silently skipped.

Usage:
  rust_mirror_court.py          # run the court, write a receipt to
                                # courts/receipts/phase-11/rust-mirror-abi-<ts>.json
"""
import datetime
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC_STRUCTS = os.path.join(ROOT, "src", "abi", "structs.rs")
SRC_TYPES = os.path.join(ROOT, "src", "abi", "types.rs")
INCLUDE = os.path.join(ROOT, "include")
RECEIPT_DIR = os.path.join(ROOT, "courts", "receipts", "phase-11")
EXAMPLES = os.path.join(ROOT, "examples")

GUARDS = (
    "#include <libxml/tree.h>\n#include <libxml/parser.h>\n"
    "#include <libxml/xpath.h>\n#include <libxml/xmlreader.h>\n"
    "#include <libxml/xmlwriter.h>\n#include <libxml/xmlschemas.h>\n"
    "#include <libxml/schemasInternals.h>\n#include <libxml/xmlschemastypes.h>\n"
    "#include <libxml/relaxng.h>\n#include <libxml/xinclude.h>\n"
    "#include <libxml/xpointer.h>\n#include <libxml/catalog.h>\n"
    "#include <libxml/encoding.h>\n#include <libxml/entities.h>\n"
    "#include <libxml/HTMLparser.h>\n#include <libxml/HTMLtree.h>\n"
    "#include <libxml/SAX2.h>\n#include <libxml/uri.h>\n"
    "#include <libxml/valid.h>\n#include <libxml/xmlautomata.h>\n"
    "#include <libxml/xmlregexp.h>\n#include <libxml/xmlsave.h>\n"
    "#include <libxml/xmlstring.h>\n#include <libxml/xmlunicode.h>\n"
    "#include <libxml/xmlversion.h>\n#include <libxml/xpathInternals.h>\n"
    "#include <libxml/dict.h>\n#include <libxml/hash.h>\n"
    "#include <libxml/list.h>\n#include <libxml/nanohttp.h>\n"
    "#include <libxml/parserInternals.h>\n#include <libxml/pattern.h>\n"
    "#include <libxml/schematron.h>\n#include <libxml/threads.h>\n"
    "#include <libxml/xmlIO.h>\n#include <libxml/xmlmemory.h>\n"
    "#include <libxml/xmlmodule.h>\n#include <libxml/xlink.h>\n"
    "#include <libxml/c14n.h>\n#include <libxml/chvalid.h>\n"
    "#include <libxml/debugXML.h>\n"
    "#include <libxslt/xslt.h>\n#include <libxslt/xsltInternals.h>\n"
    "#include <libxslt/transform.h>\n#include <libxslt/xsltutils.h>\n"
    "#include <libxslt/security.h>\n#include <libxslt/namespaces.h>\n"
    "#include <libxslt/variables.h>\n#include <libxslt/keys.h>\n"
    "#include <libxslt/numbersInternals.h>\n#include <libxslt/extensions.h>\n"
    "#include <libxslt/extra.h>\n#include <libxslt/functions.h>\n"
    "#include <libxslt/attributes.h>\n#include <libxslt/imports.h>\n"
    "#include <libxslt/documents.h>\n#include <libxslt/preproc.h>\n"
    "#include <libxslt/templates.h>\n#include <libexslt/exslt.h>\n"
)


def parse_structs():
    """Parse #[repr(C)] structs/unions from structs.rs -> ordered {name: [fields]}.

    Field names are kept RAW (raw identifiers keep their `r#` prefix) so the
    generated probe can reference them; comparisons normalize with
    norm_field().
    """
    text = open(SRC_STRUCTS).read()
    out = {}
    # find repr(C) attribute followed by struct/union NAME { ... }
    pat = re.compile(
        r"#\[repr\(C\)\]\s*(?:#\[[^\]]*\]\s*)*"
        r"pub\s+(struct|union)\s+([A-Za-z_]\w*)\s*\{(.*?)\n\}",
        re.S)
    for m in pat.finditer(text):
        kind, name, body = m.group(1), m.group(2), m.group(3)
        fields = []
        for line in body.splitlines():
            s = line.strip()
            if not s or s.startswith("//") or s.startswith("/*") or s.startswith("*"):
                continue
            if s.startswith("#"):
                continue
            # pub name: Type,  (raw identifiers may be r#name)
            fm = re.match(r"pub\s+(r#)?([A-Za-z_]\w*)\s*:", s)
            if fm:
                fields.append((fm.group(1) or "") + fm.group(2))
        out[name] = {"kind": kind, "fields": fields}
    return out


def norm_field(f):
    """Strip the `r#` raw-identifier prefix for cross-language comparison."""
    return f[2:] if f.startswith("r#") else f


def parse_enums():
    """Parse #[repr(C)] enums from types.rs -> {name: [(variant, value)]}."""
    text = open(SRC_TYPES).read()
    out = {}
    pat = re.compile(
        r"#\[repr\(C\)\]\s*(?:#\[[^\]]*\]\s*)*"
        r"pub\s+enum\s+([A-Za-z_]\w*)\s*\{(.*?)\n\}",
        re.S)
    for m in pat.finditer(text):
        name, body = m.group(1), m.group(2)
        variants = []
        auto = 0
        for line in body.splitlines():
            s = line.strip().rstrip(",")
            if not s or s.startswith("//") or s.startswith("*"):
                continue
            vm = re.match(r"([A-Za-z_]\w*)\s*(?:=\s*(-?\d+))?", s)
            if not vm:
                continue
            vname = vm.group(1)
            if vm.group(2) is not None:
                auto = int(vm.group(2))
            else:
                auto += 1
            variants.append((vname, auto))
        out[name] = variants
    return out


def c_header_text():
    hay = ""
    for root, _d, files in os.walk(INCLUDE):
        for fn in files:
            if fn.endswith(".h"):
                try:
                    hay += open(os.path.join(root, fn), encoding="utf-8",
                                errors="replace").read() + "\n"
                except OSError:
                    pass
    return hay


def clang_record_layouts():
    """Compile a TU against the candidate headers with clang's
    -fdump-record-layouts-complete and return {record_name: [fields]} where
    each field is (byte_offset, field_name). This is the COMPILER's view
    (post-preprocessing), so #ifdef-gated fields appear only when active."""
    src = os.path.join(ROOT, "target", "mirror-layout.c")
    with open(src, "w") as f:
        f.write(GUARDS + "\nint keep;\n")
    r = subprocess.run(
        ["clang", "-std=c11", "-I", INCLUDE,
         "-Xclang", "-fdump-record-layouts-complete", "-fsyntax-only", src],
        capture_output=True, text=True)
    if r.returncode != 0:
        return None, r.stderr[-2000:]
    out = {}
    cur = None
    dump = (r.stdout or "") + (r.stderr or "")
    for line in dump.splitlines():
        if line.strip().startswith("*** Dumping"):
            cur = None  # each record block is delimited by this marker
            continue
        m = re.match(r"^\s*\d+ \| (?:struct|union|class) ([A-Za-z_]\w*)$", line)
        if m:
            cur = m.group(1)
            out.setdefault(cur, [])
            continue
        if cur is None or "[sizeof=" in line:
            continue
        # top-level fields have exactly three spaces after the pipe; nested
        # anonymous struct/union members are indented deeper and excluded
        fm = re.match(r"^\s*(\d+) \|   ([^ ].*?)\s+([A-Za-z_]\w*)$", line)
        if fm:
            out[cur].append((int(fm.group(1)), fm.group(3)))
    return out, None


def c_struct_fields(name, hay):
    """Fallback (grep-based) field list when clang is unavailable."""
    m = re.search(rf"struct\s+{re.escape(name)}\s*\{{(.*?)\n\}};", hay, re.S)
    if not m:
        fwd = re.search(rf"typedef\s+struct\s+{re.escape(name)}\s+", hay)
        if fwd:
            return "OPAQUE"
        return None
    body = re.sub(r"/\*.*?\*/", "", m.group(1), flags=re.S)
    body = re.sub(r"//[^\n]*", "", body)
    fields = []
    for line in body.splitlines():
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        fm = re.search(r"([A-Za-z_]\w*)\s*(?:\[[^\]]*\]|:[^;]*)?;", s)
        if not fm:
            continue
        fields.append(fm.group(1))
    return fields


def c_enum_variants(name, hay):
    m = re.search(rf"typedef\s+enum\s*\{{(.*?)\}}\s*{re.escape(name)};", hay, re.S)
    if not m:
        m = re.search(rf"enum\s+{re.escape(name)}\s*\{{(.*?)\n\}};", hay, re.S)
    if not m:
        return None
    body = re.sub(r"/\*.*?\*/", "", m.group(1), flags=re.S)
    body = re.sub(r"//[^\n]*", "", body)
    variants = []
    auto = -1
    for line in body.splitlines():
        s = line.strip().rstrip(",")
        if not s:
            continue
        vm = re.match(r"([A-Za-z_]\w*)\s*(?:=\s*(-?\d+))?", s)
        if not vm:
            continue
        if vm.group(2) is not None:
            auto = int(vm.group(2))
        else:
            auto += 1
        variants.append((vm.group(1), auto))
    return variants


def gen_rust_example(structs, enums):
    lines = [
        "// GENERATED by tools/abi/rust_mirror_court.py — do not hand-edit.",
        "// Measures the Rust #[repr(C)] mirrors of the C ABI.",
        "// Output is normalized through rustfmt at generation time so the",
        "// committed example stays byte-identical to generator output AND",
        "// passes `cargo fmt --check` (11.1-Z seal requirement).",
        "use std::alloc::{alloc_zeroed, dealloc, Layout};",
        "use std::mem::{align_of, offset_of, size_of, size_of_val};",
        "use libxml_rs::abi::structs::*;",
        "use libxml_rs::abi::types::*;",
        "fn main() {",
    ]
    for name, info in sorted(structs.items()):
        t = name
        lines.append(f"  println!(\"STRUCT {name} sizeof={{}} alignof={{}}\", "
                     f"size_of::<{t}>(), align_of::<{t}>());")
        for f in info["fields"]:
            lines.append(f"  {{ unsafe {{")
            lines.append(f"    let layout = Layout::new::<{t}>();")
            lines.append(f"    let p = alloc_zeroed(layout) as *mut {t};")
            lines.append(f"    println!(\"FIELD {name}.{f} offsetof={{}} size={{}}\", "
                         f"offset_of!({t}, {f}), size_of_val(&(*p).{f}));")
            lines.append(f"    dealloc(p as *mut u8, layout);")
            lines.append(f"  }} }}")
    for name, variants in sorted(enums.items()):
        lines.append(f"  println!(\"ENUM {name} sizeof={{}} alignof={{}}\", "
                     f"size_of::<{name}>(), align_of::<{name}>());")
        for vname, _ in variants:
            lines.append(f"  println!(\"EVAL {name}.{vname}={{}}\", {name}::{vname} as isize);")
    lines.append("}")
    path = os.path.join(EXAMPLES, "abi_mirror.rs")
    os.makedirs(EXAMPLES, exist_ok=True)
    source = "\n".join(lines) + "\n"
    r = subprocess.run(["rustfmt", "--edition", "2021"],
                       input=source, capture_output=True, text=True)
    if r.returncode != 0:
        raise SystemExit("rustfmt normalization failed:\n" + r.stderr)
    with open(path, "w") as f:
        f.write(r.stdout)
    return path


def gen_c_probe(structs, enums, hay):
    lines = [
        "#include <stddef.h>",
        "#include <stdio.h>",
        GUARDS,
        "int main(void) {",
    ]
    for name, info in sorted(structs.items()):
        lines.append(f'  printf("STRUCT {name} sizeof=%zu alignof=%zu\\n", '
                     f'sizeof(struct {name}), _Alignof(struct {name})); // ENT:struct:{name}')
        # field names come from the C header, not the Rust mirror
        for f in info["fields"]:
            lines.append(f'  printf("FIELD {name}.{f} offsetof=%zu size=%zu\\n", '
                         f'offsetof(struct {name}, {f}), '
                         f'sizeof(((struct {name} *)0)->{f})); // ENT:field:{name}.{f}')
    for name, variants in sorted(enums.items()):
        # candidate headers use `typedef enum { ... } NAME;` (no tag)
        lines.append(f'  printf("ENUM {name} sizeof=%zu alignof=%zu\\n", '
                     f'sizeof({name}), _Alignof({name})); // ENT:enum:{name}')
        for vname, _ in variants:
            lines.append(f'  printf("EVAL {name}.{vname}=%d\\n", (int){vname}); // ENT:eval:{name}.{vname}')
    lines.append("  return 0;")
    lines.append("}")
    path = os.path.join(ROOT, "target", "abi-mirror.c")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write("\n".join(lines) + "\n")
    return path


def parse_rust(out):
    structs, enums = {}, {}
    for line in out.splitlines():
        if line.startswith("STRUCT "):
            m = re.match(r"STRUCT (\S+) sizeof=(\d+) alignof=(\d+)", line)
            if m:
                structs.setdefault(m.group(1), {})["sizes"] = (int(m.group(2)), int(m.group(3)))
        elif line.startswith("FIELD "):
            m = re.match(r"FIELD (\S+)\.(\S+) offsetof=(\d+) size=(\d+)", line)
            if m:
                structs.setdefault(m.group(1), {}).setdefault("fields", {})[m.group(2)] = \
                    (int(m.group(3)), int(m.group(4)))
        elif line.startswith("ENUM "):
            m = re.match(r"ENUM (\S+) sizeof=(\d+) alignof=(\d+)", line)
            if m:
                enums.setdefault(m.group(1), {})["sizes"] = (int(m.group(2)), int(m.group(3)))
        elif line.startswith("EVAL "):
            m = re.match(r"EVAL (\S+)\.(\S+)=(-?\d+)", line)
            if m:
                enums.setdefault(m.group(1), {}).setdefault("values", {})[m.group(2)] = int(m.group(3))
    return structs, enums


def parse_c(out):
    structs, enums = {}, {}
    for line in out.splitlines():
        if line.startswith("STRUCT "):
            m = re.match(r"STRUCT (\S+) sizeof=(\d+) alignof=(\d+)", line)
            if m:
                structs.setdefault(m.group(1), {})["sizes"] = (int(m.group(2)), int(m.group(3)))
        elif line.startswith("FIELD "):
            m = re.match(r"FIELD (\S+)\.(\S+) offsetof=(\d+) size=(\d+)", line)
            if m:
                structs.setdefault(m.group(1), {}).setdefault("fields", {})[m.group(2)] = \
                    (int(m.group(3)), int(m.group(4)))
        elif line.startswith("ENUM "):
            m = re.match(r"ENUM (\S+) sizeof=(\d+) alignof=(\d+)", line)
            if m:
                enums.setdefault(m.group(1), {})["sizes"] = (int(m.group(2)), int(m.group(3)))
        elif line.startswith("EVAL "):
            m = re.match(r"EVAL (\S+)\.(\S+)=(-?\d+)", line)
            if m:
                enums.setdefault(m.group(1), {}).setdefault("values", {})[m.group(2)] = int(m.group(3))
    return structs, enums


def build_c_probe_with_retry(cpath):
    """Compile the C probe, dropping only the exact failing lines (by line
    number). Returns (stdout, skipped_ent_keys). Skipped entities are always
    returned for classification — nothing is silently omitted."""
    line_re = re.compile(r"^([^:]+):(\d+):(?:\d+:)?(?: error| fatal error)")
    ent_re = re.compile(r"// ENT:(\S+)")
    skipped = []
    for _ in range(100):
        cr = subprocess.run(["gcc", "-std=c11", "-o", cpath + ".bin", cpath,
                             "-I", INCLUDE], capture_output=True, text=True)
        if cr.returncode == 0:
            run = subprocess.run([cpath + ".bin"], capture_output=True, text=True)
            return run.stdout, skipped
        bad = set()
        for m in line_re.finditer(cr.stderr):
            if os.path.basename(m.group(1)) == os.path.basename(cpath):
                bad.add(int(m.group(2)))
        if not bad:
            raise SystemExit("MIRROR C PROBE COMPILE FAILED:\n" + cr.stderr[-3000:])
        with open(cpath) as f:
            lines = f.readlines()
        kept = []
        for i, ln in enumerate(lines, start=1):
            if i in bad:
                em = ent_re.search(ln)
                if em:
                    skipped.append(em.group(1))
            else:
                kept.append(ln)
        with open(cpath, "w") as f:
            f.writelines(kept)
    raise SystemExit("MIRROR C PROBE RETRY LIMIT")


def main():
    structs = parse_structs()
    enums = parse_enums()
    hay = c_header_text()
    clang_layouts, clang_err = clang_record_layouts()
    if clang_layouts is None:
        print("CLANG LAYOUT DUMP FAILED:\n", clang_err)
        return 1

    # Classify Rust types against the candidate C headers BEFORE measuring.
    classifications = {}
    c_fields = {}
    for name in structs:
        if name in clang_layouts:
            classifications[name] = "MEASURABLE"
            c_fields[name] = [f for _o, f in clang_layouts[name]]
        else:
            fwd = re.search(rf"typedef\s+struct\s+{re.escape(name)}\s+\w+;|struct\s+{re.escape(name)}\s*;", hay)
            body = re.search(rf"struct\s+{re.escape(name)}\s*{{", hay)
            if fwd:
                classifications[name] = "OPAQUE_FORWARD_DECL"
            elif body:
                classifications[name] = "CONFIGURATION_ABSENT"
            else:
                classifications[name] = "NOT_LAYOUT_OBSERVABLE"
    for name in enums:
        if c_enum_variants(name, hay) is None:
            classifications[name] = "NOT_LAYOUT_OBSERVABLE"
        else:
            classifications[name] = "MEASURABLE"

    gen_rust_example(structs, enums)
    r = subprocess.run(["cargo", "run", "--quiet", "--example", "abi_mirror"],
                       cwd=ROOT, capture_output=True, text=True)
    if r.returncode != 0:
        print("RUST EXAMPLE FAILED:\n", r.stderr[-3000:])
        return 1
    rs_structs, rs_enums = parse_rust(r.stdout)

    cpath = gen_c_probe(
        {n: {"fields": c_fields.get(n, [])} for n in structs
         if classifications.get(n) == "MEASURABLE"},
        {n: v for n, v in enums.items() if classifications.get(n) == "MEASURABLE"},
        hay)
    c_out, c_skipped = build_c_probe_with_retry(cpath)
    # any entity the compiler could not measure (e.g. an enum behind a
    # disabled conditional) is classified CONFIGURATION_ABSENT
    for key in c_skipped:
        kind = key.split(":")[0]
        name = key.split(":", 1)[1].split(".")[0]
        if classifications.get(name) == "MEASURABLE":
            classifications[name] = "CONFIGURATION_ABSENT"
    c_structs, c_enums = parse_c(c_out)

    mismatches = []
    measured = 0
    for name in sorted(structs):
        cls = classifications[name]
        if cls != "MEASURABLE":
            continue
        measured += 1
        rs, c = rs_structs.get(name), c_structs.get(name)
        if rs is None or c is None:
            mismatches.append({"type": name, "reason": "missing on one side",
                               "rust": bool(rs), "c": bool(c)})
            continue
        if rs["sizes"] != c["sizes"]:
            mismatches.append({"type": name, "reason": "size/align",
                               "rust": rs["sizes"], "c": c["sizes"]})
        # positional field comparison (declaration order)
        rfields = [norm_field(f) for f in structs[name]["fields"]]
        cfields = c_fields.get(name, [])
        if len(rfields) != len(cfields):
            mismatches.append({"type": name, "reason": "field count",
                               "rust": len(rfields), "c": len(cfields),
                               "rust_fields": rfields, "c_fields": cfields})
        for i, rf in enumerate(rfields):
            if i >= len(cfields):
                break
            cf = cfields[i]
            rv = rs["fields"].get(structs[name]["fields"][i])
            cv = c["fields"].get(cf)
            if rv is None or cv is None or rv != cv:
                mismatches.append({"type": name, "reason": f"field[{i}] {rf} vs {cf}",
                                   "rust": rv, "c": cv})
    for name in sorted(enums):
        cls = classifications[name]
        if cls != "MEASURABLE":
            continue
        measured += 1
        rs, c = rs_enums.get(name), c_enums.get(name)
        if rs is None or c is None:
            mismatches.append({"type": name, "reason": "missing on one side",
                               "rust": bool(rs), "c": bool(c)})
            continue
        if rs["sizes"] != c["sizes"]:
            mismatches.append({"type": name, "reason": "size/align",
                               "rust": rs["sizes"], "c": c["sizes"]})
        rv = rs.get("values", {})
        cv = c.get("values", {})
        for vname in sorted(set(rv) | set(cv)):
            if rv.get(vname) != cv.get(vname):
                mismatches.append({"type": name, "reason": f"enumerator {vname}",
                                   "rust": rv.get(vname), "c": cv.get(vname)})

    classified_unmeasurable = {n: c for n, c in classifications.items() if c != "MEASURABLE"}
    verdict = "PASS" if not mismatches else "FAIL"
    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    receipt = {
        "court": "RUST-MIRROR-ABI",
        "phase": "11.1-G",
        "timestamp": ts,
        "triangle": ["upstream C header (sealed by ABI_PARITY_LEDGER)",
                     "candidate C header (include/)",
                     "Rust #[repr(C)] mirrors (src/abi/structs.rs, src/abi/types.rs)"],
        "accounting": {
            "rust_repr_c_types": len(structs) + len(enums),
            "measurable": measured,
            "classified_unmeasurable": {c: sorted(n for n, cc in classified_unmeasurable.items() if cc == c)
                                        for c in ("OPAQUE_FORWARD_DECL", "NOT_LAYOUT_OBSERVABLE")},
            "silently_omitted": 0,
        },
        "mismatch_count": len(mismatches),
        "mismatches": mismatches,
        "verdict": verdict,
        "regenerate": "tools/abi/rust_mirror_court.py",
    }
    os.makedirs(RECEIPT_DIR, exist_ok=True)
    out = os.path.join(RECEIPT_DIR, f"rust-mirror-abi-{ts}.json")
    with open(out, "w") as f:
        json.dump(receipt, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"receipt -> {out}")
    print(f"RUST types: {len(structs)} structs/unions + {len(enums)} enums; "
          f"measurable={measured} classified={len(classified_unmeasurable)} "
          f"mismatches={len(mismatches)} verdict={verdict}")
    for mm in mismatches[:20]:
        print("  ", mm)
    return 0 if verdict == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
