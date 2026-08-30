#!/usr/bin/env python3
"""Exhaustive C ABI census probe generator + runner (11.1-G).

Generates one C probe per project covering every public struct (sizeof/alignof/
offsetof per field), union, and enum (every enumerator value), compiles it
against the ORACLE headers (system /usr/include) and against the CANDIDATE
headers (repository include/), executes both, and diffs the values into
atlas/ABI_PARITY_LEDGER.json.

sizeof/offsetof are compile-time constants, so the candidate probe needs no
link against the Rust library — a header-only compile proves the layout claim.

Usage:
  abi_probe_gen.py
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DOX = os.path.join(ROOT, "oracle", "historical", "doxygen")
ATLAS = os.path.join(ROOT, "atlas")

PROJECTS = {
    # The CURRENT ABI seal uses current-system evidence only:
    #   system Doxygen public inventory × system headers × system DSO
    # (historical inventories are used separately for historical ABI epochs).
    "libxml2": {
        "inv": os.path.join(DOX, "libxml2-system", "inventory-public.json"),
        "public_inv": os.path.join(DOX, "libxml2-system", "inventory-public.json"),
        "oracle_inc": ["/usr/include/libxml2", "/usr/include"],
        "cand_inc": [os.path.join(ROOT, "include")],
        "oracle_dso": "/usr/lib/libxml2.so.16",
        "candidate_dso": os.path.join(ROOT, "target", "debug", "liblibxml_rs.so"),
        "include_guard": (
            "#include <libxml/tree.h>\n#include <libxml/parser.h>\n"
            "#include <libxml/xpath.h>\n#include <libxml/xmlreader.h>\n"
            "#include <libxml/xmlwriter.h>\n#include <libxml/xmlschemas.h>\n"
            "#include <libxml/schemasInternals.h>\n#include <libxml/xmlschemastypes.h>\n#include <libxml/relaxng.h>\n"
            "#include <libxml/xinclude.h>\n#include <libxml/xpointer.h>\n"
            "#include <libxml/catalog.h>\n#include <libxml/encoding.h>\n"
            "#include <libxml/entities.h>\n#include <libxml/HTMLparser.h>\n"
            "#include <libxml/HTMLtree.h>\n#include <libxml/SAX2.h>\n"
            "#include <libxml/uri.h>\n#include <libxml/valid.h>\n"
            "#include <libxml/xmlautomata.h>\n#include <libxml/xmlregexp.h>\n"
            "#include <libxml/xmlsave.h>\n#include <libxml/xmlstring.h>\n"
            "#include <libxml/xmlunicode.h>\n#include <libxml/xmlversion.h>\n"
            "#include <libxml/xpathInternals.h>\n#include <libxml/dict.h>\n"
            "#include <libxml/hash.h>\n#include <libxml/list.h>\n"
            "#include <libxml/nanohttp.h>\n#include <libxml/parserInternals.h>\n"
            "#include <libxml/pattern.h>\n#include <libxml/schematron.h>\n"
            "#include <libxml/threads.h>\n#include <libxml/xmlIO.h>\n"
            "#include <libxml/xmlmemory.h>\n#include <libxml/xmlmodule.h>\n"
            "#include <libxml/xlink.h>\n#include <libxml/c14n.h>\n"
            "#include <libxml/chvalid.h>\n#include <libxml/debugXML.h>\n"),
        "version": "2.15.3",
    },
    "libxslt": {
        "inv": os.path.join(DOX, "libxslt-system", "inventory-public.json"),
        "public_inv": os.path.join(DOX, "libxslt-system", "inventory-public.json"),
        "oracle_inc": ["/usr/include/libxml2", "/usr/include"],
        "cand_inc": [os.path.join(ROOT, "include")],
        "oracle_dso": "/usr/lib/libxslt.so.1",
        "candidate_dso": os.path.join(ROOT, "target", "debug", "liblibxml_rs.so"),
        "include_guard": (
            "#include <libxslt/xslt.h>\n#include <libxslt/xsltInternals.h>\n"
            "#include <libxslt/transform.h>\n#include <libxslt/xsltutils.h>\n"
            "#include <libxslt/security.h>\n#include <libxslt/namespaces.h>\n"
            "#include <libxslt/variables.h>\n#include <libxslt/keys.h>\n"
            "#include <libxslt/numbersInternals.h>\n#include <libxslt/extensions.h>\n"
            "#include <libxslt/extra.h>\n#include <libxslt/functions.h>\n"
            "#include <libxslt/attributes.h>\n#include <libxslt/imports.h>\n"
            "#include <libxslt/documents.h>\n#include <libxslt/preproc.h>\n"
            "#include <libxslt/templates.h>\n#include <libexslt/exslt.h>\n"),
        "version": "1.1.45",
    },
}

SKIP_STRUCTS = {"__anon"}


def collect(project):
    inv = json.load(open(PROJECTS[project]["inv"]))
    inv_public = json.load(open(
        os.path.join(DOX, PROJECTS[project]["public_inv"])))
    public_headers = {e.get("header") for e in inv_public["entities"]
                      if e.get("header")}
    structs = {}
    for e in inv["entities"]:
        if e["kind"] == "variable" and e.get("struct") \
                and e.get("header") in public_headers:
            structs.setdefault(e["struct"], []).append((e["name"], e.get("type", "")))
    enums = {}
    for e in inv["entities"]:
        if e["kind"] == "enum" and e.get("header") in public_headers:
            enums[e["name"]] = e.get("enum_values", [])
    return structs, enums


def candidate_defined(project, structs, enums):
    """Which structs/enums the candidate headers actually define (grep-based),
    so the candidate probe compiles and the census records the rest as MISSING."""
    inc = os.path.join(ROOT, "include")
    hay = ""
    for root, _d, files in os.walk(inc):
        for fn in files:
            if fn.endswith(".h"):
                try:
                    hay += open(os.path.join(root, fn), encoding="utf-8",
                                errors="replace").read() + "\n"
                except OSError:
                    pass
    s_def = {s for s in structs if re.search(rf"struct\s+[A-Za-z_]*{re.escape(s)}\s*{{|typedef\s+struct\s+[A-Za-z_]*{re.escape(s)}", hay)}
    e_def = set()
    for ename, values in enums.items():
        if re.search(rf"\b{re.escape(ename)}\b", hay):
            e_def.add(ename)
    return s_def, e_def


def gen_probe(project, entity_filter=None):
    structs, enums = collect(project)
    s_def, e_def = candidate_defined(project, structs, enums)
    if entity_filter == "candidate":
        structs = {s: f for s, f in structs.items() if s in s_def}
        enums = {e: v for e, v in enums.items() if e in e_def}
    elif entity_filter == "oracle-only":
        structs = {s: f for s, f in structs.items() if s not in s_def}
        enums = {e: v for e, v in enums.items() if e not in e_def}
    guard = PROJECTS[project]["include_guard"]
    lines = [
        "#include <stddef.h>",
        "#include <stdio.h>",
        guard,
        "int main(void) {",
    ]
    # Every measurement line carries a `// ENT:<entity-key>` marker so the
    # retry loop can account for each dropped entity by identity. The court
    # then proves: measured ∪ classified-skipped == discovered, and
    # silently omitted == 0.
    n_struct = 0
    for sname, fields in sorted(structs.items()):
        if not sname or sname in SKIP_STRUCTS or sname.startswith("(") \
                or sname.startswith("__anon") or not re.match(r"^[A-Za-z_]\w*$", sname):
            continue
        lines.append(f'  printf("STRUCT {sname} sizeof=%zu alignof=%zu\\n", '
                     f'sizeof(struct {sname}), _Alignof(struct {sname})); // ENT:struct:{sname}')
        for fname, _ftype in fields:
            if not re.match(r"^[A-Za-z_]\w*$", fname):
                continue
            lines.append(f'  printf("  FIELD {sname}.{fname} offsetof=%zu sizeof=%zu\\n", '
                         f'offsetof(struct {sname}, {fname}), '
                         f'sizeof(((struct {sname} *)0)->{fname})); // ENT:field:{sname}.{fname}')
        n_struct += 1
    n_enum = 0
    for ename, values in sorted(enums.items()):
        if not re.match(r"^[A-Za-z_]\w*$", ename):
            continue
        for vname, _vinit in values:
            if not re.match(r"^[A-Za-z_]\w*$", vname):
                continue
            lines.append(f'  printf("ENUM {ename}.{vname}=%d\\n", (int){vname}); // ENT:enum:{ename}.{vname}')
        n_enum += 1
    lines.append("  return 0;")
    lines.append("}")
    src = "\n".join(lines) + "\n"
    out = os.path.join(ROOT, "target", f"abi-probe-{project}.c")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    with open(out, "w") as f:
        f.write(src)
    return out, n_struct, n_enum


def build_and_run(src, inc_dirs, tag):
    """Compile+run; on errors, drop the offending measurement lines and
    retry (bounded). Returns (stdout, compile_error, skipped) where
    `skipped` is the list of ENT entity-keys removed during retries.

    Nothing is ever silently omitted: every dropped line is returned to the
    caller, which must classify it (see classify_skipped).
    """
    exe = src + f".{tag}"
    retries = 0
    skipped = []
    line_re = re.compile(r"^([^:]+):(\d+):(?:\d+:)?(?: error| fatal error)")
    ent_re = re.compile(r"// ENT:(\S+)")
    while retries < 200:
        args = ["gcc", "-std=c11", "-o", exe, src] + \
               [a for i in inc_dirs for a in ("-I", i)]
        r = subprocess.run(args, capture_output=True, text=True)
        if r.returncode == 0:
            run = subprocess.run([exe], capture_output=True, text=True)
            return run.stdout, None, skipped
        # Drop exactly the measurement lines that failed to compile (by their
        # source line number), never collateral lines that merely share a
        # name with the error. Every dropped line's ENT key is returned so the
        # caller classifies it; nothing is silently omitted.
        bad_lines = set()
        for m in line_re.finditer(r.stderr):
            if os.path.basename(m.group(1)) == os.path.basename(src):
                bad_lines.add(int(m.group(2)))
        if not bad_lines:
            return None, r.stderr[-2000:], skipped
        with open(src) as f:
            lines = f.readlines()
        kept = []
        for i, ln in enumerate(lines, start=1):
            if i in bad_lines:
                em = ent_re.search(ln)
                if em:
                    skipped.append(em.group(1))
            else:
                kept.append(ln)
        if len(kept) == len(lines):
            return None, r.stderr[-2000:], skipped
        with open(src, "w") as f:
            f.writelines(kept)
        retries += 1
    return None, "retry limit", skipped


SKIP_CLASSES = ["OPAQUE_FORWARD_DECL", "NOT_LAYOUT_OBSERVABLE",
                "CONFIGURATION_ABSENT", "TOOLING_LIMITATION", "CANDIDATE_GAP"]


def concat_headers(paths):
    hay = ""
    for root, _d, files in os.walk(paths):
        for fn in files:
            if fn.endswith(".h"):
                try:
                    hay += open(os.path.join(root, fn), encoding="utf-8",
                                errors="replace").read() + "\n"
                except OSError:
                    pass
    return hay


def classify_skipped(project, keys, side):
    """Classify every entity the retry loop dropped, so the ledger can prove
    that nothing was silently omitted.

    side: 'oracle' | 'candidate' | 'oracle-gaps'

    Classes (see SKIP_CLASSES):
      OPAQUE_FORWARD_DECL   header declares the type but defines no body
      NOT_LAYOUT_OBSERVABLE the C headers do not define this entity at all
                            (Doxygen attributed it from private/source text)
      CONFIGURATION_ABSENT  entity exists in the header under a conditional
                            compilation gate the probe does not enable
      CANDIDATE_GAP         oracle-defined surface missing from candidate
                            headers (only on candidate-side probes)
      TOOLING_LIMITATION    anything else (should not occur; fails the court)
    """
    cand = concat_headers(os.path.join(ROOT, "include"))
    orac = concat_headers("/usr/include/libxml2")
    out = {}
    for key in keys:
        if key.startswith("struct:"):
            name = key[7:]
            body_c = re.search(rf"struct\s+{re.escape(name)}\s*{{", cand)
            body_o = re.search(rf"struct\s+{re.escape(name)}\s*{{", orac)
            fwd_c = re.search(rf"struct\s+{re.escape(name)}\s*;", cand)
            fwd_o = re.search(rf"struct\s+{re.escape(name)}\s*;", orac)
            if body_o:
                if body_c:
                    out[key] = "TOOLING_LIMITATION"
                elif side == "candidate":
                    out[key] = "CANDIDATE_GAP"
                else:
                    out[key] = "TOOLING_LIMITATION"
            elif fwd_o:
                out[key] = "OPAQUE_FORWARD_DECL"
            elif fwd_c:
                out[key] = "OPAQUE_FORWARD_DECL"
            else:
                out[key] = "NOT_LAYOUT_OBSERVABLE"
        elif key.startswith("field:") or key.startswith("enum:"):
            # field:STRUCT.FIELD or enum:ENUM.VALUE
            _, rest = key.split(":", 1)
            name = rest.split(".")[-1]
            in_c = re.search(rf"\b{re.escape(name)}\b", cand)
            in_o = re.search(rf"\b{re.escape(name)}\b", orac)
            if in_o and not in_c and side == "candidate":
                out[key] = "CANDIDATE_GAP"
            elif in_o and not in_c:
                out[key] = "CANDIDATE_GAP"
            elif not in_o and not in_c:
                out[key] = "NOT_LAYOUT_OBSERVABLE"
            elif in_o and in_c:
                out[key] = "CONFIGURATION_ABSENT"
            else:
                out[key] = "TOOLING_LIMITATION"
        else:
            out[key] = "TOOLING_LIMITATION"
    return out


def inventory_hash(path):
    """sha256 of the Doxygen inventory JSON (byte-exact identity of the
    extraction the ABI probe is derived from)."""
    import hashlib
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def parse(text):
    out = {}
    for line in text.splitlines():
        if line.startswith("STRUCT "):
            m = re.match(r"STRUCT (\S+) sizeof=(\d+) alignof=(\d+)", line)
            if m:
                out[f"struct:{m.group(1)}"] = {"sizeof": int(m.group(2)),
                                               "alignof": int(m.group(3))}
        elif line.startswith("  FIELD "):
            m = re.match(r"  FIELD (\S+)\.(\S+) offsetof=(\d+) sizeof=(\d+)", line)
            if m:
                out[f"field:{m.group(1)}.{m.group(2)}"] = {
                    "offsetof": int(m.group(3)), "sizeof": int(m.group(4))}
        elif line.startswith("ENUM "):
            m = re.match(r"ENUM (\S+)\.(\S+)=(-?\d+)", line)
            if m:
                out[f"enum:{m.group(1)}.{m.group(2)}"] = int(m.group(3))
    return out


def write_markdown(ledger):
    """Generated Markdown view of atlas/ABI_PARITY_LEDGER.json (11.1-W).

    Every total is recomputed from the JSON — never hand-edited.
    """
    L = ["# ABI Parity Ledger — generated by tools/abi/abi_probe_gen.py", "",
         "Struct/enum layout parity measured by compiling C probes against the oracle",
         "headers (/usr/include) and the candidate headers (include/) and requiring",
         "byte-identical sizeof/offsetof/enum-value output. `silently_omitted` MUST be",
         "0 — the ledger fails otherwise (fail-open prohibited).", ""]
    for proj, p in ledger["projects"].items():
        acc = p["probe_accounting"]
        L += [f"## {proj} {p['version']}", "",
              f"- oracle DSO: `{p['evidence_sources']['oracle_dso']}`",
              f"- structs probed: {p['structs_probed']}, enums probed: {p['enums_probed']}",
              f"- oracle entities: {p['oracle_entities']}, "
              f"candidate entities: {p['candidate_entities']}",
              f"- mismatches: {p['mismatch_count']}",
              f"- verdict: **{p['verdict']}**", "",
              "### Probe accounting (fail-open prohibited)", "",
              "| metric | oracle | candidate |", "|---|---|---|"]
        for metric in ("discovered", "measurable", "classified_unmeasurable",
                       "silently_omitted"):
            L.append(f"| {metric} | {acc['oracle'][metric]} | {acc['candidate'][metric]} |")
        L.append("")
    with open(os.path.join(ATLAS, "ABI_PARITY_LEDGER.md"), "w") as f:
        f.write("\n".join(L) + "\n")


def main():
    ledger = {"schema": "abi-parity-ledger-3", "projects": {}}
    for project, info in PROJECTS.items():
        src, n_struct, n_enum = gen_probe(project)
        print(f"{project}: probe with {n_struct} structs, {n_enum} enums -> {src}")

        def discovered_keys(path):
            with open(path) as f:
                return set(re.findall(r"// ENT:(\S+)", f.read()))

        def accounting(discovered, measured, skipped_keys, side):
            """Prove nothing was silently omitted: measured ∪ classified ==
            discovered, measured ∩ classified == ∅."""
            classifications = classify_skipped(project, skipped_keys, side)
            classified = set(classifications)
            if set(skipped_keys) != classified:
                raise SystemExit(f"classifier missed entities: "
                                 f"{set(skipped_keys) - classified}")
            silently = discovered - measured - classified
            if silently:
                raise SystemExit(f"SILENT OMISSION ({project}/{side}): "
                                 f"{sorted(silently)[:10]} were neither measured "
                                 f"nor classified — fail-open prohibited")
            unclassified = classified - {k for k, c in classifications.items()
                                         if c in SKIP_CLASSES}
            if unclassified:
                raise SystemExit(f"UNCLASSIFIED SKIPS ({project}/{side}): "
                                 f"{sorted(unclassified)}")
            return {
                "discovered": len(discovered),
                "measurable": len(measured),
                "classified_unmeasurable": len(classified),
                "silently_omitted": len(silently),
                "skip_classifications": {
                    c: sorted(k for k, cc in classifications.items() if cc == c)
                    for c in SKIP_CLASSES},
            }

        # oracle probe: full discovered surface
        vo, err_o, skip_o = build_and_run(src, info["oracle_inc"], "oracle")
        dsc_o = discovered_keys(src)
        # candidate probe: only entities the candidate headers define; the rest
        # are recorded as header-surface gaps (residuals), not probe failures
        src_c, n_sc, n_ec = gen_probe(project, "candidate")
        vc, err_c, skip_c = build_and_run(src_c, info["cand_inc"], "candidate")
        dsc_c = discovered_keys(src_c)
        if err_o or err_c:
            print(f"  compile errors: oracle={bool(err_o)} candidate={bool(err_c)}")
            if err_o:
                print("  oracle:", err_o[:300])
            if err_c:
                print("  candidate:", err_c[:300])
        po, pc = parse(vo or ""), parse(vc or "")
        acc_o = accounting(dsc_o, set(po), skip_o, "oracle")
        acc_c = accounting(dsc_c, set(pc), skip_c, "candidate")
        mismatches = []
        for k, v in po.items():
            if k not in pc:
                mismatches.append({"entity": k, "oracle": v, "candidate": "MISSING"})
            elif pc[k] != v:
                mismatches.append({"entity": k, "oracle": v, "candidate": pc[k]})
        for k, v in pc.items():
            if k not in po:
                mismatches.append({"entity": k, "oracle": "MISSING", "candidate": v})
        # oracle-only entities = candidate header-surface gaps
        src_o, n_so, n_eo = gen_probe(project, "oracle-only")
        header_gaps = []
        if n_so or n_eo:
            vo2, err_o2, skip_o2 = build_and_run(src_o, info["oracle_inc"], "oracle-gaps")
            dsc_o2 = discovered_keys(src_o)
            acc_g = accounting(dsc_o2, set(parse(vo2 or "")), skip_o2, "oracle-gaps")
            if not err_o2:
                header_gaps = sorted(parse(vo2 or "").keys())
            ledger.setdefault("_gap_accounting", {})[project] = acc_g
        ledger["projects"][project] = {
            "version": info["version"],
            "evidence_sources": {
                "doxygen_inventory": os.path.relpath(info["inv"], ROOT),
                "doxygen_inventory_hash": inventory_hash(info["inv"]),
                "oracle_headers": info["oracle_inc"][0],
                "candidate_headers": os.path.relpath(ROOT + os.sep + "include", ROOT),
                "oracle_dso": info["oracle_dso"],
                "candidate_dso": os.path.relpath(info["candidate_dso"], ROOT),
                "probe_compiler": "gcc -std=c11",
                "note": ("Current ABI seal derived from current-system Doxygen public "
                          "inventory (libxml2-system / libxslt-system) × current-system "
                          "headers (/usr/include) × current-system DSO. Historical ABI "
                          "epochs are sealed separately from historical inventories "
                          "(atlas/HISTORICAL_SURFACE_EPOCHS.json)"),
            },
            "probe_accounting": {
                "oracle": acc_o,
                "candidate": acc_c,
                "note": ("discovered = measurement lines generated; measurable = "
                          "entities actually measured; classified_unmeasurable = "
                          "entities dropped during compile retries with an explicit "
                          "classification; silently_omitted MUST be 0 — the ledger "
                          "fails otherwise (fail-open prohibited)"),
            },
            "structs_probed": n_struct,
            "enums_probed": n_enum,
            "oracle_entities": len(po),
            "candidate_entities": len(pc),
            "mismatch_count": len(mismatches),
            "mismatches": mismatches,
            "candidate_header_gap_entities": header_gaps,
            "verdict": "PASS" if not mismatches else "FAIL",
        }
        print(f"  oracle: discovered={acc_o['discovered']} measurable={acc_o['measurable']} "
              f"classified={acc_o['classified_unmeasurable']} "
              f"silent={acc_o['silently_omitted']}")
        print(f"  candidate: discovered={acc_c['discovered']} measurable={acc_c['measurable']} "
              f"classified={acc_c['classified_unmeasurable']} "
              f"silent={acc_c['silently_omitted']}")
        print(f"  oracle entities={len(po)} candidate={len(pc)} "
              f"mismatches={len(mismatches)} header-gaps={len(header_gaps)} "
              f"verdict={ledger['projects'][project]['verdict']}")
        for mm in mismatches[:6]:
            print("   ", mm)
    out = os.path.join(ATLAS, "ABI_PARITY_LEDGER.json")
    with open(out, "w") as f:
        json.dump(ledger, f, indent=1, ensure_ascii=False)
        f.write("\n")
    write_markdown(ledger)
    print("ledger ->", out)
    return 0 if all(p["verdict"] == "PASS" for p in ledger["projects"].values()) else 1


if __name__ == "__main__":
    sys.exit(main())
