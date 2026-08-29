#!/usr/bin/env python3
"""11.1-G/H court-driven header closure.

The candidate `include/` headers are the drop-in C interface. Every public
struct/enum that the ABI census (`tools/abi/abi_probe_gen.py`) reports MISSING
in the candidate must be defined with byte-exact upstream layout.

This tool extracts those definitions verbatim from the corresponding upstream
oracle header (libxml2-2.15.0 / libxslt-1.1.42 archaeology trees) and manages
them inside a single regenerated section of each candidate header:

    /* [11.1-G] begin: definitions extracted verbatim from upstream oracle */
    <forward typedefs>
    <enums>
    <function-pointer / plain typedefs>
    <struct bodies>
    /* [11.1-G] end */

The section is fully rebuilt on every run (idempotent), and a compile-fix loop
resolves cascading missing types until the whole include set compiles. The ABI
probe then verifies layout equality.

Verbatim extraction is the point: hand-retranscribing struct layouts is how
drift gets introduced. The upstream headers are the ABI contract being
reimplemented, so their public record/enum definitions are interface
specification, not implementation code.

Usage:
  header_closure.py [--project libxml2|libxslt] [--only NAME,...]
"""
import argparse
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

UPSTREAM = {
    "libxml2": os.path.join(ROOT, "oracle", "historical", "src",
                            "libxml2-2.15.0", "include", "libxml"),
    "libxslt": os.path.join(ROOT, "oracle", "historical", "src",
                            "libxslt-1.1.42", "libxslt"),
}
CAND = {
    "libxml2": os.path.join(ROOT, "include", "libxml"),
    "libxslt": os.path.join(ROOT, "include", "libxslt"),
}
EXSLT_UPSTREAM = os.path.join(ROOT, "oracle", "historical", "src",
                              "libxslt-1.1.42", "libexslt")

INCLUDE_SETS = {
    "libxml2": [
        "libxml/tree.h", "libxml/parser.h", "libxml/xpath.h",
        "libxml/xmlreader.h", "libxml/xmlwriter.h", "libxml/xmlschemas.h",
        "libxml/xmlschemastypes.h", "libxml/relaxng.h", "libxml/xinclude.h",
        "libxml/xpointer.h", "libxml/catalog.h", "libxml/encoding.h",
        "libxml/entities.h", "libxml/HTMLparser.h", "libxml/HTMLtree.h",
        "libxml/SAX2.h", "libxml/uri.h", "libxml/valid.h",
        "libxml/xmlautomata.h", "libxml/xmlregexp.h", "libxml/xmlsave.h",
        "libxml/xmlstring.h", "libxml/xmlunicode.h", "libxml/xmlversion.h",
        "libxml/xpathInternals.h", "libxml/dict.h", "libxml/hash.h",
        "libxml/list.h", "libxml/nanohttp.h", "libxml/parserInternals.h",
        "libxml/pattern.h", "libxml/schematron.h", "libxml/threads.h",
        "libxml/xmlIO.h", "libxml/xmlmemory.h", "libxml/xmlmodule.h",
    ],
    "libxslt": [
        "libxslt/xslt.h", "libxslt/xsltInternals.h", "libxslt/transform.h",
        "libxslt/xsltutils.h", "libxslt/security.h", "libxslt/namespaces.h",
        "libxslt/variables.h", "libxslt/keys.h", "libxslt/numbersInternals.h",
        "libxslt/extensions.h", "libxslt/extra.h", "libxslt/functions.h",
        "libxslt/attributes.h", "libxslt/imports.h", "libxslt/documents.h",
        "libxslt/preproc.h", "libxslt/templates.h", "libexslt/exslt.h",
    ],
}

BEGIN = "/* [11.1-G] begin: extracted verbatim from upstream oracle header */"
END = "/* [11.1-G] end: extracted definitions */"

_cache = {}


def upstream_headers(project):
    out = {}
    for fn in os.listdir(UPSTREAM[project]):
        if fn.endswith(".h"):
            out[fn] = os.path.join(UPSTREAM[project], fn)
    if project == "libxslt":
        for fn in os.listdir(EXSLT_UPSTREAM):
            if fn.endswith(".h"):
                out[fn] = os.path.join(EXSLT_UPSTREAM, fn)
    return out


def upstream_text(path):
    if path not in _cache:
        _cache[path] = open(path, encoding="utf-8", errors="replace").read()
    return _cache[path]


def strip_comments_block(text):
    out = []
    in_block = False
    for ln in text.splitlines():
        if in_block:
            if "*/" in ln:
                in_block = False
            continue
        s = ln.strip()
        if s.startswith("/*"):
            if "*/" not in ln:
                in_block = True
            continue
        out.append(ln)
    return "\n".join(out)


def find_braced_block(text, start_idx):
    depth = 0
    i = start_idx
    n = len(text)
    while i < n:
        c = text[i]
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1, text[start_idx:i + 1]
        i += 1
    return None, None


def extract_entity(project, name):
    """Extract definition of `name` from upstream oracle headers.

    Returns (kind, pre, body, post, basename) where kind is one of
    'fwd' (typedef struct _x x;), 'enum', 'typedef' (plain/function-pointer),
    'struct' (struct body). pre/post are attached typedefs that must be
    emitted before the body. Returns (None,)*5 when not found.
    """
    headers = upstream_headers(project)
    # 1) named struct tag, with or without underscore prefix
    for tag in (("_" + name) if not name.startswith("_") else name, name):
        for basename, path in sorted(headers.items()):
            text = upstream_text(path)
            m = re.search(rf"\bstruct\s+{re.escape(tag)}\s*\{{", text)
            if m:
                end, block = find_braced_block(text, m.end() - 1)
                if end is None:
                    continue
                head = text[:m.start()]
                pre = ""
                pm = list(re.finditer(r"typedef\s+struct\s+_\w+\s+(\w+);", head))
                if pm and pm[-1].group(1) == name:
                    pre = head[pm[-1].start():].strip() + "\n"
                tail = text[end:]
                post = ""
                tm = re.match(rf"\s*typedef\s+{re.escape(name)}\s*\*\s*(\w+)\s*;",
                              tail)
                if tm:
                    post = tail[:tm.end()].strip() + "\n"
                # block ends at `}`; C needs the terminating `;`
                body = "struct " + tag + " " + block.rstrip()
                if not body.endswith(";"):
                    body += ";"
                body += "\n"
                return "struct", pre, body, post, basename
    # 2) forward typedef: `typedef struct _N N;`
    for basename, path in sorted(headers.items()):
        text = upstream_text(path)
        m = re.search(
            rf"typedef\s+struct\s+_{re.escape(name)}\s+{re.escape(name)}\s*;",
            text)
        if m:
            return "fwd", "", m.group(0).strip() + "\n", "", basename
    # 3) named enum: `typedef enum { ... } name;`
    for basename, path in sorted(headers.items()):
        text = upstream_text(path)
        for m in re.finditer(r"\btypedef\s+enum\s*\{", text):
            end, block = find_braced_block(text, m.end() - 1)
            if end is None:
                continue
            tail = text[end:]
            tm = re.match(rf"\s*({re.escape(name)})\s*;", tail)
            if tm:
                # block includes the closing brace; keep it once
                body = strip_comments_block(
                    "typedef enum" + block.rstrip()[:-1] + "} " + name + ";")
                return "enum", "", body + "\n", "", basename
    # 4) object-like / function-like macros: `#define NAME ...`
    for basename, path in sorted(headers.items()):
        text = upstream_text(path)
        m = re.search(rf"^\s*#\s*define\s+{re.escape(name)}\b[^\n]*(?:\\\n[^\n]*)*",
                      text, re.M)
        if m:
            return "", m.group(0).strip() + "\n", "", basename
    # 5) plain typedef where `name` is the declared identifier
    for basename, path in sorted(headers.items()):
        text = upstream_text(path)
        for m in re.finditer(
                rf"\btypedef\s+(?!struct\b)(?!enum\b)[^;]*?\b{re.escape(name)}\b[^;]*;",
                text, re.S):
            seg = m.group(0)
            if re.search(rf"\b{re.escape(name)}\b\s*\)?\s*(?:\([^;]*)?;", seg):
                return "typedef", "", strip_comments_block(seg.strip()) + "\n", \
                    "", basename
    return None, None, None, None, None


def candidate_header_path(project, basename):
    if basename.startswith("exslt"):
        return os.path.join(ROOT, "include", "libexslt", basename)
    return os.path.join(CAND[project], basename)


def repair_guard(header_path):
    """Restore the `#ifdef __cplusplus / extern "C" { / #endif` triplet that
    earlier tool iterations clobbered (they replaced the opening `#ifdef` with
    content, swallowing the declarations into the C++ branch)."""
    with open(header_path) as f:
        c = f.read()
    m = re.search(r'#ifdef __cplusplus\s*\n\s*extern "C" \{\s*\n(?!\s*#endif)', c)
    if m:
        c = c[:m.end()] + "#endif\n" + c[m.end():]
        with open(header_path, "w") as f:
            f.write(c)
        print(f"  [repair-guard] {os.path.basename(header_path)}")
        return True
    return False


def upstream_fwd_typedefs(project, basename, existing_fwd):
    """Extract ALL forward typedefs from the upstream header
    (`typedef struct _x x;` and `typedef x *xPtr;` pairs), skipping any that
    the section itself already provides (declared in `existing_fwd`) so that
    no typedef is redefined (clang -Wtypedef-redefinition in C89/C99)."""
    path = upstream_headers(project).get(basename)
    if not path:
        return ""
    text = upstream_text(path)
    out = []
    for ln in text.splitlines():
        s = ln.strip()
        if re.match(r"typedef\s+struct\s+_\w+\s+\w+\s*;", s):
            if s not in existing_fwd:
                out.append(s)
        elif re.match(r"typedef\s+\w+\s*\*\s*\w+\s*;", s):
            # pointer typedef `typedef X *XPtr;` — skip when the base type's
            # own fwd typedef is already provided (the section emits it in
            # the right order: fwd before pointer typedef)
            m = re.match(r"typedef\s+(\w+)\s*\*\s*\w+\s*;", s)
            base = m.group(1) if m else ""
            if base and any(
                    t == f"typedef struct _\w+ {base};"
                    or t == f"typedef struct _{base} {base};"
                    for t in existing_fwd):
                continue
            if s not in existing_fwd:
                out.append(s)
    if not out:
        return ""
    # dedupe, preserve first-seen order
    seen = set()
    uniq = []
    for s in out:
        if s not in seen:
            seen.add(s)
            uniq.append(s)
    return "\n".join(uniq) + "\n"


def candidate_global_typedefs(project):
    """All `typedef struct _x x;` / `typedef x *xPtr;` lines present in the
    candidate headers (hand-written content and existing sections)."""
    out = set()
    roots = [CAND[project]]
    exslt = os.path.join(ROOT, "include", "libexslt")
    if os.path.isdir(exslt):
        roots.append(exslt)
    for root in roots:
        for fn in os.listdir(root):
            if not fn.endswith(".h"):
                continue
            c = open(os.path.join(root, fn),
                     encoding="utf-8", errors="replace").read()
            for ln in c.splitlines():
                s = ln.strip()
                if re.match(r"typedef\s+struct\s+_\w+\s+\w+\s*;", s) or \
                        re.match(r"typedef\s+\w+\s*\*\s*\w+\s*;", s):
                    out.add(s)
    return out


def regenerate_section(project, header_path, entries, global_typedefs=None):
    """Rebuild the [11.1-G] section of a header from `entries`:
    list of (kind, text). Groups: fwd, enum, typedef, struct (deterministic
    order, declare-before-use). Returns True if the header changed."""
    repair_guard(header_path)
    with open(header_path) as f:
        content = f.read()
    b, e = content.find(BEGIN), content.find(END)
    if b != -1 and e != -1 and e > b:
        content = content[:b] + content[e + len(END):]
    if not entries:
        if b != -1:
            with open(header_path, "w") as f:
                f.write(content)
            print(f"  [section-clear] {os.path.basename(header_path)}")
            return True
        return False
    def key(kind):
        return {"fwd": 0, "enum": 1, "typedef": 2, "struct": 3}[kind]
    # dedupe identical definition text (a struct can be resolved under both
    # `_xmlX` and `xmlX` names)
    seen_text = set()
    uniq = []
    for kind, text in entries:
        t = text.strip()
        if t in seen_text:
            continue
        seen_text.add(t)
        uniq.append((kind, t))
    parts = sorted(uniq, key=lambda kv: (key(kv[0]), kv[1]))
    existing_fwd = {t for k, t in uniq if k == "fwd"}
    if global_typedefs:
        existing_fwd |= global_typedefs
    basename = os.path.basename(header_path)
    fwd = upstream_fwd_typedefs(project, basename, existing_fwd)
    block = "\n" + BEGIN + "\n"
    if fwd:
        block += fwd.rstrip("\n") + "\n\n"
    for kind, text in parts:
        block += text.rstrip("\n") + "\n\n"
    block += END + "\n"
    # place the section at the END of the declarations (before the closing
    # `#ifdef __cplusplus` / `#endif`), so the header's own forward typedefs
    # (xmlNode, xmlNs, ...) are already declared when the section runs
    idx = content.rfind("#ifdef __cplusplus")
    if idx == -1:
        idx = content.rfind("#endif")
    content = content[:idx] + block + content[idx:]
    with open(header_path, "w") as f:
        f.write(content)
    print(f"  [section] {os.path.basename(header_path)}: "
          f"{len(parts)} definitions")
    return True


def remove_define_conflicts(header_path, consts):
    """Remove `#define CONST value` lines now provided by extracted enums."""
    with open(header_path) as f:
        lines = f.readlines()
    keep = []
    removed = 0
    for ln in lines:
        m = re.match(r"\s*#\s*define\s+(\w+)\b", ln)
        if m and m.group(1) in consts:
            removed += 1
            continue
        keep.append(ln)
    if removed:
        with open(header_path, "w") as f:
            f.writelines(keep)
        print(f"  [remove-defines] {os.path.basename(header_path)}: "
              f"{removed} #define(s) migrated to enums")


def compile_set(project):
    src = "#include <stddef.h>\n"
    for inc in INCLUDE_SETS[project]:
        src += f"#include <{inc}>\n"
    src += "int main(void){ return 0; }\n"
    p = os.path.join(ROOT, "target", f"header-closure-{project}.c")
    os.makedirs(os.path.dirname(p), exist_ok=True)
    with open(p, "w") as f:
        f.write(src)
    args = ["gcc", "-std=c11", "-fsyntax-only", p,
            "-I", os.path.join(ROOT, "include")]
    r = subprocess.run(args, capture_output=True, text=True)
    return r.returncode, r.stderr


def missing_type_names(stderr):
    names = set()
    stderr = (stderr.replace("\u201c", "'")
              .replace("\u201d", "'")
              .replace("\u2018", "'")
              .replace("\u2019", "'"))
    for m in re.finditer(
            r"unknown type name '(\w+)'|'(\w+)' undeclared|"
            r"incomplete type 'struct (\w+)'|"
            r"field '(\w+)' has incomplete type", stderr):
        names.update(g for g in m.groups() if g)
    return names


def candidate_defines(project, name):
    """True if the candidate headers' HAND-WRITTEN content (outside the
    [11.1-G] section) already defines `name`. The section is excluded so its
    own definitions never mask ledger-missing entities."""
    hay = ""
    roots = [CAND[project]]
    exslt = os.path.join(ROOT, "include", "libexslt")
    if os.path.isdir(exslt):
        roots.append(exslt)
    for root in roots:
        for fn in os.listdir(root):
            if fn.endswith(".h"):
                try:
                    c = open(os.path.join(root, fn),
                             encoding="utf-8", errors="replace").read()
                except OSError:
                    continue
                b, e = c.find(BEGIN), c.find(END)
                if b != -1 and e != -1 and e > b:
                    c = c[:b] + c[e + len(END):]
                hay += c + "\n"
    if re.search(rf"struct\s+_{re.escape(name)}\s*\{{|struct\s+{re.escape(name)}\s*\{{", hay):
        return True
    if re.search(rf"\btypedef\s+enum\s*\{{[^}}]*\}}\s*{re.escape(name)}\s*;", hay, re.S):
        return True
    if re.search(rf"\btypedef\s+[^;]*?\b{re.escape(name)}\b\s*\)?\s*(?:\([^;]*)?;", hay, re.S):
        return True
    return False


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--project", choices=["libxml2", "libxslt"], default=None)
    ap.add_argument("--only", default="")
    args = ap.parse_args()

    projects = ["libxml2", "libxslt"] if not args.project else [args.project]
    only = {x.strip() for x in args.only.split(",") if x.strip()}

    for project in projects:
        print(f"== {project} ==")
        missing = set()
        ledger = os.path.join(ROOT, "atlas", "ABI_PARITY_LEDGER.json")
        if os.path.exists(ledger):
            import json
            d = json.load(open(ledger))
            for m in d["projects"].get(project, {}).get("mismatches", []):
                e = m["entity"]
                if e.startswith("struct:"):
                    missing.add(e[7:])
                elif e.startswith("enum:"):
                    missing.add(e[5:].split(".")[0])
        if only:
            missing |= only
        # drop entities the candidate ALREADY defines in hand-written content
        # (stale-ledger guard)
        missing = {n for n in missing if not candidate_defines(project, n)}
        if not missing:
            print("  ledger-missing entities already defined; clearing stale sections")

        # (kind, pre, body, post, basename) per entity, keyed by name
        resolved = {}

        # seed `resolved` with entities currently present in existing sections
        # so regeneration is idempotent across probe-refresh cycles
        for root in ([CAND[project]] + [os.path.join(ROOT, "include", "libexslt")]):
            if not os.path.isdir(root):
                continue
            for fn in sorted(os.listdir(root)):
                if not fn.endswith(".h"):
                    continue
                c = open(os.path.join(root, fn),
                         encoding="utf-8", errors="replace").read()
                b, e = c.find(BEGIN), c.find(END)
                if b == -1 or e == -1 or e <= b:
                    continue
                seg = c[b:e]
                for m in re.finditer(r"struct\s+(_?\w+)\s*\{", seg):
                    n = m.group(1)
                    if n not in resolved:
                        kind, pre, body, post, basename = extract_entity(project, n)
                        if body:
                            resolved[n] = (kind, pre or "", body, post or "", basename)
                # fwd typedefs inside the section: `typedef struct _x x;`
                for m in re.finditer(r"typedef\s+struct\s+(_?\w+)\s+(\w+)\s*;", seg):
                    n = m.group(2)
                    if n not in resolved:
                        kind, pre, body, post, basename = extract_entity(project, n)
                        if body:
                            resolved[n] = (kind, pre or "", body, post or "", basename)
                for m in re.finditer(r"\}\s*(xml\w+|html\w+|xslt\w+)\s*;", seg):
                    n = m.group(1)
                    if n not in resolved:
                        kind, pre, body, post, basename = extract_entity(project, n)
                        if body:
                            resolved[n] = (kind, pre or "", body, post or "", basename)
        if not resolved and not missing:
            continue
        for name in list(missing):
            kind, pre, body, post, basename = extract_entity(project, name)
            if not body:
                print(f"  [NOT-FOUND] {name}")
                continue
            resolved.setdefault(name, (kind, pre or "", body, post or "", basename))

        # enum constants per enum for #define migration
        enum_consts = set()
        for name, (kind, _pre, body, _post, _b) in resolved.items():
            if kind == "enum":
                consts = re.findall(r"\b(XML_\w+|XSLT_\w+|XS_\w+)\s*(?:=|\s*[,}])",
                                    body)
                enum_consts.update(consts)

        # Compile-iterate: regenerate sections, add cascading missing types
        for _round in range(60):
            # group per candidate header
            per_header = {}
            gtd = candidate_global_typedefs(project)
            for name, (kind, pre, body, post, basename) in resolved.items():
                hpath = candidate_header_path(project, basename)
                if not hpath:
                    continue
                per_header.setdefault(hpath, [])
                # skip fwd/pointer typedefs that already exist elsewhere in
                # the candidate (clang -Wtypedef-redefinition in C89/C99)
                def not_global(t):
                    return t.strip() not in gtd or t.strip() in {x[1] for x in per_header[hpath]}
                if pre and not_global(pre):
                    per_header[hpath].append(("fwd", pre))
                if body:
                    per_header[hpath].append((kind, body))
                if post and not_global(post):
                    per_header[hpath].append(("typedef", post))
            changed = False
            gtd = candidate_global_typedefs(project)
            for hpath, entries in sorted(per_header.items()):
                os.makedirs(os.path.dirname(hpath), exist_ok=True)
                if regenerate_section(project, hpath, entries, gtd):
                    changed = True
                # migrate #define constants once enums are in place
                if enum_consts:
                    remove_define_conflicts(hpath, enum_consts)
            # clear stale sections in headers that no longer need entries
            for root in ([CAND[project]] + [os.path.join(ROOT, "include", "libexslt")]):
                if not os.path.isdir(root):
                    continue
                for fn in sorted(os.listdir(root)):
                    if not fn.endswith(".h"):
                        continue
                    hpath = os.path.join(root, fn)
                    if hpath in per_header:
                        continue
                    with open(hpath) as f:
                        c = f.read()
                    if BEGIN in c:
                        if regenerate_section(project, hpath, [], gtd):
                            changed = True
            rc, err = compile_set(project)
            if rc == 0:
                print(f"  compile: OK ({project})")
                break
            names = missing_type_names(err)
            if not names:
                print("  compile: UNRESOLVED ERROR")
                print(err[:4000])
                break
            new = {n for n in names if n not in resolved}
            if not new:
                print("  compile: no new names; remaining errors:")
                print(err[:3000])
                break
            for n in sorted(new):
                kind, pre, body, post, basename = extract_entity(project, n)
                if not body:
                    print(f"  [MISSING-TYPE] {n} (not found upstream)")
                    # record as failed so the loop terminates
                    resolved[n] = ("failed", "", "", "", None)
                    continue
                resolved[n] = (kind, pre or "", body, post or "", basename)
            if not changed:
                print("  (no section regeneration needed this round)")
        else:
            print(f"  compile: ROUND LIMIT ({project})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
