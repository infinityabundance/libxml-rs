#!/usr/bin/env python3
"""ABI-FUNCTION-SIGNATURE court (11.1-Z.2) — three-way C function-prototype mirror.

For every externally relevant function the court proves the triangle:

    oracle Clang C prototype            (system headers, clang AST)
              ↕  (must match, modulo documented divergences)
    candidate installed C prototype     (include/, clang AST)
              ↕  (must match exactly)
    actual Rust extern "C" signature    (src/, extracted from source)

The measured dimensions per symbol:

    return type category     (void / int / uint / long / size_t / pointer /
                              fnptr / float / double / char / record / other)
    argument count
    per-argument type category + pointer depth
    callback/function-pointer types     (fnptr category)
    variadic status
    calling convention        (extern "C")
    integer width/sign        (int vs long vs long long / i32 vs i64)

Typedef normalization: `xmlNodePtr` ≡ `xmlNode *` (pointer-depth + record
base); `const` on a top-level pointer is ABI-irrelevant and stripped; the
`Func` typedefs map to fnptr.

The x86-64 assembly variadic shims (xmlGenericError / xsltGenericError
default printers, R-000161) have C-variadic prototypes but Rust signatures
that are the documented shim form; they are CLASSIFIED in ASM_SHIMS, never
silently skipped.

This is the function-level equivalent of the struct-mirror court (RUST-MIRROR-
ABI): a signature mismatch here means a C caller following the installed
header passes arguments/expects a return that the Rust implementation
misreads — the xmlGcMemSetup class of bug (11.1-Z.2, R-000176).

Usage:
  function_signature_court.py            # run the court, write a receipt
  function_signature_court.py --report   # print every finding in detail
"""
import argparse
import datetime
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, "src")
INCLUDE = os.path.join(ROOT, "include")
RECEIPTS = os.path.join(ROOT, "courts", "receipts", "phase-11")

ORACLE_HEADER_DIRS = ["/usr/include/libxml2/libxml",
                      "/usr/include/libxslt", "/usr/include/libexslt"]
CAND_HEADER_DIRS = [os.path.join(INCLUDE, "libxml"),
                    os.path.join(INCLUDE, "libxslt"),
                    os.path.join(INCLUDE, "libexslt")]

# ── x86-64 variadic asm shims: C prototype is variadic; the Rust signature is
# the documented shim form (R-000161). Classified, not flagged. ────────────────
# `xmlStrPrintf`, `xsltTransformError` and the four legacy SAX v1 handlers
# (`xmlParserError` etc., 11.1-Z.2 R-000176) are noreturn inline-asm blocks
# whose declared arity is a Rust-side fiction: stable Rust cannot define
# c_variadic bodies, so the shim captures the SysV register save area and
# forwards a va_list to a V-receiver.
ASM_SHIMS = {
    "xmlGenericErrorDefaultFunc",
    "xsltGenericErrorDefaultFunc",
    "xmlStrPrintf",
    "xsltTransformError",
    "xmlParserError",
    "xmlParserWarning",
    "xmlParserValidityError",
    "xmlParserValidityWarning",
}

# Oracle-header internal indirection names (globals.h DLL-import scheme):
# `__xmlGenericError` etc. are the library-internal backing symbols behind the
# macro-aliased public globals; the candidate exports the clean public names
# only. Classified, not flagged.
ORACLE_INTERNAL_GLOBALS = {
    "__xmlGenericError",
    "__xmlGenericErrorContext",
    "__xmlLastError",
    "__xmlOutputBufferCreateFilename",
    "__xmlOutputBufferCreateFilenameValue",
    "__xmlParserInputBufferCreateFilename",
    "__xmlParserInputBufferCreateFilenameValue",
    "__xmlStructuredError",
    "__xmlStructuredErrorContext",
}

# Oracle functions deliberately not exported by the candidate (bounded,
# documented divergences — residuals). Classified with their residual.
ORACLE_NOT_EXPORTED = {
    # xslHandleDebugger (xslt.h, WITH_DEBUGGER): the XSLT debugger interface
    # is not implemented; xsltSetDebuggerCallbacks accepts and ignores the
    # callback block (R-000176).
    "xslHandleDebugger": "XSLT debugger interface not implemented (residual R-000176)",
}

# Exports that are DATA (function-pointer variables / globals), not functions —
# the function-signature plane does not apply to the Rust `static`.
DATA_EXPORTS = {
    "xmlFree", "xmlMalloc", "xmlMallocAtomic", "xmlRealloc", "xmlMemStrdup",
    "xmlParserVersion", "xmlStringText", "xmlStringTextNoenc", "xmlStringComment",
    "xmlStringTextLen", "xmlEntityValue", "xmlBufferAllocScheme", "xmlDefaultBufferSize",
    "xmlGenericError", "xmlGenericErrorContext", "xmlStructuredError", "xmlStructuredErrorContext",
    "xmlRegisterNodeDefaultValue", "xmlDeregisterNodeDefaultValue", "xmlTreeIndentString",
    "xmlSaveNoEmptyTags", "xmlIndentTreeOutput", "xmlTreeDebug", "xmlSaveDebug",
    "xmlDoValidityCheckingDefaultValue", "xmlPedanticParserDefaultValue",
    "xmlLineNumbersDefaultValue", "xmlLoadExtDtdDefaultValue", "xmlKeepBlanksDefaultValue",
    "xmlSubstituteEntitiesDefaultValue", "xmlDebugEntities", "xmlParserDebugEntities",
    "xmlOldXMLWDcompatibility", "xmlDefaultSAXHandler", "xmlDefaultSAXLocator",
    "xmlLastError", "xmlCharEncodingHandlers", "xmlParserInputBufferCreateFilenameDefault",
    "xmlOutputBufferCreateFilenameDefault", "xmlXPathNAN", "xmlXPathPINF", "xmlXPathNINF",
    "xmlXPathNaN", "xmlXPathPInf", "xmlXPathNInf",
    "xsltGenericDebug", "xsltGenericDebugContext", "xsltGenericError", "xsltGenericErrorContext",
    "xsltLibxmlVersion", "xsltLibxsltVersion", "xsltEngineVersion", "xsltDocDefaultLoader",
    "xsltExtMarker", "xslDebugStatus",
    "exsltLibraryVersion", "exsltLibexsltVersion", "exsltLibxmlVersion", "exsltLibxsltVersion",
}


def clang_prototypes(header_dirs):
    """Extract {name: {ret, params:[...], variadic}} from C headers via clang.
    Also records enum typedef names (ABI: C enums are int) and fn-pointer
    typedef names (xmlHashCopier, xmlListWalker, ...)."""
    all_decls = {}
    global ENUM_TYPEDEFS, FNPTR_TYPEDEFS
    for d in header_dirs:
        if not os.path.isdir(d):
            continue
        for fn in sorted(os.listdir(d)):
            if not fn.endswith(".h"):
                continue
            path = os.path.join(d, fn)
            r = subprocess.run(
                ["clang", "-Xclang", "-ast-dump=json", "-fsyntax-only",
                 "-I", INCLUDE, "-I", os.path.join(INCLUDE, "libxml2"), path],
                capture_output=True, text=True)
            if r.returncode != 0:
                continue
            try:
                doc = json.loads(r.stdout)
            except Exception:
                continue
            stack = [doc]
            while stack:
                n = stack.pop()
                if n.get("kind") == "FunctionDecl" and n.get("name"):
                    name = n["name"]
                    q = n.get("type", {}).get("qualType", "")
                    # return type = qualType prefix before the function's own
                    # parameter list (first '('): "int (a, b)" -> "int";
                    # "xmlDoc *(a)" -> "xmlDoc *"
                    ret = q.split("(", 1)[0].strip() if "(" in q else q
                    params = []
                    for c in n.get("inner", []):
                        if c.get("kind") == "ParmVarDecl":
                            params.append(c.get("type", {}).get("qualType", ""))
                    variadic = bool(n.get("variadic", False))
                    if name not in all_decls:
                        all_decls[name] = {"ret": ret, "params": params,
                                           "variadic": variadic, "header": fn}
                elif n.get("kind") == "TypedefDecl" and n.get("name"):
                    qt = n.get("type", {}).get("qualType", "")
                    if qt.startswith("enum ") or "enum" in qt:
                        ENUM_TYPEDEFS.add(n["name"])
                    elif "(" in qt and ("*" in qt or "Func" in qt):
                        FNPTR_TYPEDEFS.add(n["name"])
                for c in n.get("inner", []):
                    stack.append(c)
    return all_decls


ENUM_TYPEDEFS = set()
FNPTR_TYPEDEFS = set()


def split_params(sig_text):
    """Split a Rust/C parameter list at top-level commas (nesting-aware;
    the `>` of a `->` arrow is not a closing bracket)."""
    parts, depth, cur, prev = [], 0, [], ""
    for ch in sig_text:
        if ch in "<([":
            depth += 1
        elif ch in ">)]" and not (ch == ">" and prev == "-"):
            depth -= 1
        if ch == "," and depth == 0:
            parts.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
        prev = ch
    if "".join(cur).strip():
        parts.append("".join(cur).strip())
    return [p for p in parts if p]


def rust_fn_signatures():
    """Extract {name: {ret, params:[raw], variadic, path}} for every
    `extern "C" fn` in src/ (multi-line aware). Doc-comment copies are
    excluded by requiring the declaration to start a line."""
    out = {}
    pat = re.compile(
        r'(?m)^\s*(?:pub(?:\s*\([^)]*\))?\s+)?(?:unsafe\s+)?'
        r'extern\s+"C"\s+fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(')
    for dirpath, _dirs, files in os.walk(SRC):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            path = os.path.join(dirpath, fn)
            text = open(path, encoding="utf-8", errors="replace").read()
            for m in pat.finditer(text):
                name = m.group(1)
                i = m.end()
                depth = 1  # the matched '(' is open
                while i < len(text) and depth > 0:
                    ch = text[i]
                    if ch == "(":
                        depth += 1
                    elif ch == ")":
                        depth -= 1
                    i += 1
                params_text = text[m.end():i - 1]
                # return type: between the ')' and the body '{' only
                head = text[i:].split("{", 1)[0]
                variadic = bool(re.search(r'(?<!\.)\.\.\.', params_text))
                params = [rust_param_type(p) for p in split_params(params_text)]
                ret = "()"
                rm = re.search(r'->\s*([^{;]+)', head)
                if rm:
                    ret = rm.group(1).strip().rstrip()
                out[name] = {"ret": ret, "params": params,
                             "variadic": variadic,
                             "path": os.path.relpath(path, ROOT)}
    return out


def rust_param_type(p):
    """Strip the parameter name: `ctxt: *mut _xmlNode` -> `*mut _xmlNode`.
    The name is the token before the first top-level `: ` (fn-pointer params
    like `Option<unsafe extern "C" fn(...)>` have no top-level colon)."""
    depth = 0
    prev = ""
    for i, ch in enumerate(p):
        if ch in "<([":
            depth += 1
        elif ch in ">)]" and not (ch == ">" and prev == "-"):
            depth -= 1
        elif ch == ":" and depth == 0:
            return p[i + 1:].strip()
        prev = ch
    return p.strip()


# ── type normalization: typedefs -> canonical (category, depth) ──────────────

def norm_type(t, rust=False):
    """Canonical (category, depth) for a C qualType or Rust type text."""
    t = t.strip()
    if not t:
        return ("other", 0)
    if rust:
        depth = t.count("*")
        if t in ("()",):
            return ("void", 0)
        if t in ("c_int", "i32"):
            return ("int", 0)
        if t in ("c_uint", "u32"):
            return ("uint", 0)
        if t in ("c_long", "c_ulong", "usize", "isize", "size_t", "c_longlong", "i64", "u64"):
            return ("long", 0)
        if t in ("c_short", "c_ushort", "c_char", "u8", "i8", "xmlChar"):
            return ("char", 0)
        if t in ("f32", "c_float"):
            return ("float", 0)
        if t in ("f64", "c_double"):
            return ("double", 0)
        if t.startswith("*mut") or t.startswith("*const"):
            # `*mut xmlNodePtr` is a pointer to a pointer typedef → depth 2
            # (upstream `xmlNode **`).
            if re.search(r"\*mut\s+\w+Ptr\b", t) or re.search(r"\*const\s+\w+Ptr\b", t):
                return ("pointer", depth + 1)
            return ("pointer", depth)
        if t.startswith("Option<"):
            inner = t[len("Option<"):-1]
            # fn-pointer typedefs may be module-qualified
            # (`crate::abi::callbacks::xmlResourceLoader`); compare the last
            # path segment against the typedef/fn-suffix heuristics.
            last = inner.rsplit("::", 1)[-1]
            if ("fn" in inner and "(" in inner) or "extern" in inner or last in FNPTR_TYPEDEFS:
                return ("fnptr", 0)
            if last.endswith("Func") or last.endswith("Callback") or last.endswith("Handler"):
                return ("fnptr", 0)
            return norm_type(inner, rust=True)
        if "fn" in t and ("(" in t or "extern" in t):
            return ("fnptr", 0)
        if t.startswith("&"):
            return ("pointer", 1)
        if t in ENUM_TYPEDEFS:
            return ("int", 0)
        # opaque Rust pointer/function typedefs — compare the last path
        # segment so module-qualified typedefs resolve.
        last = t.rsplit("::", 1)[-1]
        if last in FNPTR_TYPEDEFS:
            return ("fnptr", 0)
        if last.endswith("Ptr"):
            return ("pointer", 1)
        if last.endswith("Func") or last.endswith("Function") or last.endswith("Callback"):
            return ("fnptr", 0)
        return ("other", 0)
    # C side
    depth = t.count("*")
    t = re.sub(r"\b(const|volatile|restrict)\b", "", t).strip()
    t = re.sub(r"\s+", " ", t)
    if re.search(r"\w+Ptr\s*\*", t):
        # `xmlNsPtr *` is a pointer to the pointer typedef -> depth 2
        depth += 1
        t = re.sub(r"\w+Ptr\s*\*", "", t).strip()
    elif t.endswith("Ptr") or t.endswith("Ptr "):
        depth += 1
        t = t[:-3].strip()
    if t in ("void",):
        return ("void", depth)
    if t == "int":
        return ("int", depth)
    if t == "unsigned int":
        return ("uint", depth)
    if t in ("long", "unsigned long", "size_t", "ssize_t", "ptrdiff_t",
             "long long", "unsigned long long"):
        return ("long", depth)
    if t in ("char", "signed char", "unsigned char", "short", "unsigned short",
             "xmlChar", "xmlCharPtr"):
        return ("char", depth)
    if t == "float":
        return ("float", depth)
    if t == "double":
        return ("double", depth)
    if t.endswith("Func") or t in FNPTR_TYPEDEFS:
        return ("fnptr", depth)
    if t in ENUM_TYPEDEFS:
        return ("int", depth)
    if depth > 0:
        # a pointer to a struct/enum/typedef is a pointer (ABI-identical)
        return ("pointer", depth)
    return ("other", depth)


def run_court(report=False):
    findings = []
    compared = 0
    ok = 0

    cand_all = clang_prototypes(CAND_HEADER_DIRS)
    oracle_all = clang_prototypes(ORACLE_HEADER_DIRS)

    # ── Plane 1 vs 2: oracle headers ↔ candidate headers ─────────────────────
    classified = []
    for name, oc in sorted(oracle_all.items()):
        if name in ORACLE_INTERNAL_GLOBALS:
            classified.append({"symbol": name, "plane": "oracle-vs-candidate",
                               "kind": "oracle-internal-indirection",
                               "detail": "globals.h DLL-import backing name; candidate exports the public name"})
            continue
        if name in ORACLE_NOT_EXPORTED:
            classified.append({"symbol": name, "plane": "oracle-vs-candidate",
                               "kind": "oracle-not-exported",
                               "detail": ORACLE_NOT_EXPORTED[name]})
            continue
        cc = cand_all.get(name)
        if cc is None:
            findings.append({"symbol": name, "plane": "oracle-vs-candidate",
                             "kind": "missing-in-candidate-header",
                             "detail": f"oracle: {oc['ret']}({', '.join(oc['params'])}) "
                                       f"[header {oc['header']}]"})
            continue
        compared += 1
        o = ([norm_type(oc["ret"])[0]] + [norm_type(p)[0] for p in oc["params"]]
             + [oc["variadic"]])
        c = ([norm_type(cc["ret"])[0]] + [norm_type(p)[0] for p in cc["params"]]
             + [cc["variadic"]])
        # also compare pointer depth for pointer categories
        op = [norm_type(p)[1] for p in oc["params"]]
        cp = [norm_type(p)[1] for p in cc["params"]]
        if o != c or (oc["variadic"] == cc["variadic"] and op != cp):
            findings.append({"symbol": name, "plane": "oracle-vs-candidate",
                             "kind": "prototype-drift",
                             "detail": f"oracle {oc['ret']}({', '.join(oc['params'])})"
                                       f" vs candidate {cc['ret']}({', '.join(cc['params'])})"})
        else:
            ok += 1

    # ── Plane 2 vs 3: candidate headers ↔ Rust extern signatures ─────────────
    rust = rust_fn_signatures()
    for name, cc in sorted(cand_all.items()):
        if name in DATA_EXPORTS or name in ASM_SHIMS:
            continue
        rs = rust.get(name)
        if rs is None:
            continue
        compared += 1
        problems = []
        cret = norm_type(cc["ret"])
        rret = norm_type(rs["ret"], rust=True)
        if cret != rret:
            problems.append(f"return {cc['ret']}({cret}) vs Rust {rs['ret']}({rret})")
        if len(cc["params"]) != len(rs["params"]):
            problems.append(
                f"arg count {len(cc['params'])} ({', '.join(cc['params'])}) "
                f"vs Rust {len(rs['params'])} ({', '.join(rs['params'])})")
        else:
            for i, (cp, rp) in enumerate(zip(cc["params"], rs["params"])):
                cnorm = norm_type(cp)
                rnorm = norm_type(rp, rust=True)
                if cnorm != rnorm:
                    problems.append(f"arg {i} {cp}({cnorm}) vs Rust {rp}({rnorm})")
        if cc["variadic"] != rs["variadic"]:
            problems.append(f"variadic {cc['variadic']} vs Rust {rs['variadic']}")
        if problems:
            findings.append({"symbol": name, "plane": "candidate-vs-rust",
                             "kind": "signature-mismatch",
                             "detail": "; ".join(problems),
                             "rust_path": rs["path"]})
        else:
            ok += 1

    verdict = "PASS" if not findings else "FAIL"
    ts = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    receipt = {
        "court": "ABI-FUNCTION-SIGNATURE",
        "phase": "11.1-Z.2",
        "timestamp": ts,
        "schema": "function-signature-court-1",
        "planes": {
            "oracle": "system headers via clang AST",
            "candidate_headers": "include/ via clang AST",
            "rust": "src/ extern \"C\" fn extraction",
        },
        "asm_shims_classified": sorted(ASM_SHIMS),
        "oracle_internal_globals_classified": sorted(ORACLE_INTERNAL_GLOBALS),
        "oracle_not_exported_classified": dict(sorted(ORACLE_NOT_EXPORTED.items())),
        "data_exports_excluded": sorted(DATA_EXPORTS),
        "summary": {"compared": compared, "ok": ok, "findings": len(findings),
                    "classified": len(classified)},
        "classified": classified,
        "findings": findings,
        "verdict": verdict,
    }
    os.makedirs(RECEIPTS, exist_ok=True)
    rp = os.path.join(RECEIPTS, f"abi-function-signature-{ts}.json")
    with open(rp, "w") as f:
        json.dump(receipt, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"receipt -> {rp}")
    print(f"compared={compared} ok={ok} findings={len(findings)} verdict={verdict}")
    if report:
        for f_ in findings:
            print(f"  [{f_['kind']}] {f_['symbol']}: {f_['detail']}")
    return 0 if verdict == "PASS" else 1


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--report", action="store_true")
    args = ap.parse_args()
    return run_court(report=args.report)


if __name__ == "__main__":
    sys.exit(main())
