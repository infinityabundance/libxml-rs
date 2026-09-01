#!/usr/bin/env python3
"""Add the Phase-13 hostile-audit residuals to RESIDUAL_LEDGER.json (Phase 13).

The Phase-13 hostile courts attacked the candidate with NULLs, extremes and
UB-adjacent defined behavior, and each court family's findings became a
residual (FIXED with the court as permanent regression protection). The
ledger is the only hand-maintained truth; ledger_gen.py regenerates the
Markdown.

Usage:
    python3 tools/phase13/residualize.py
"""
import datetime
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LEDGER = os.path.join(ROOT, "atlas", "RESIDUAL_LEDGER.json")

TODAY = datetime.date.today().isoformat()

NEW = [
    {
        "id": "R-000190",
        "status": "FIXED",
        "phase": "13",
        "title": "HOSTILE-THREADS: the error-handler slots (xmlGenericError/Context, xmlStructuredError/Context) and the other 14 TLS-era globals were GLOBAL data instead of thread-local (upstream 2.15 LIBXML_THREAD_ENABLED xmlGetThreadLocalStorage model)",
        "surface": "Error-handler/parser-default/node-hook/IO-hook globals (globals.c 2.15, xmlerror.h/parser.h/xmlIO.h/tree.h)",
        "component": [
            "src/xml/globals/tls.rs",
            "src/xml/globals/mod.rs",
            "src/abi/data_globals.rs",
            "include/libxml/globals.h",
            "include/libxml/parser.h",
            "include/libxml/xmlerror.h",
            "include/libxml/xmlIO.h",
            "include/libxml/tree.h",
        ],
        "discovery_date": TODAY,
        "fixed_date": TODAY,
        "root_cause": "The candidate exported the 18 TLS-era globals as plain data symbols and the __xml* accessors returned addr_of_mut! of them; upstream 2.15 with LIBXML_THREAD_ENABLED keeps them in per-thread storage (globals.c xmlGetThreadLocalStorage) and the oracle DSO exports ONLY the __xml* accessor FUNCTIONS. A handler installed in one thread was therefore observable from every other thread (HOSTILE-THREADS T2: after the workers installed a no-op structured handler, the main thread's own diagnostics were swallowed on the candidate while the oracle still printed them).",
        "fix": "Moved all 18 TLS-era globals into thread_local! cells (src/xml/globals/tls.rs), single source of truth per thread; the __xml* accessors now return pointers into the current thread's cells; removed the 18 plain data-symbol exports and switched the candidate headers to the upstream macro/accessor contract (#define xmlXxx (*__xmlXxx())); regenerated the export-surface disposition (libxml2 CUSTODIAN_EXTENSION 45->31).",
        "observable_residual": "None: xmlSetStructuredErrorFunc/xmlSetGenericErrorFunc are thread-scoped exactly like the oracle; the xmlLastError global mirror (R-000135) is unchanged.",
        "triangulation": "HOSTILE-THREADS probe (T1 concurrent parses, T2 error isolation, T3 concurrent global reads) byte-identical vs the system oracle; GLOBALS-THREADING and DATA-GLOBALS-001 courts still byte-identical; ABI-FUNCTION-SIGNATURE 3277/3277.",
        "regression_courts": [
            "HOSTILE-THREADS",
            "GLOBALS-THREADING-001",
            "DATA-GLOBALS-001",
            "ABI-FUNCTION-SIGNATURE",
            "HEADER-COMPILE",
            "EXPORT-SURFACE-DISPOSITION",
        ],
        "evidence": [
            "courts/receipts/phase-13/hostile-threads-*.json",
            "src/xml/globals/tls.rs",
        ],
        "classification": "CANDIDATE_BUG",
        "history": [
            {"status": "OPEN", "date": TODAY,
             "note": "HOSTILE-THREADS probe failed: candidate stderr empty where oracle printed the main-thread T2 error — workers' TLS-scoped structured handler was globally visible on the candidate"},
            {"status": "FIXED", "date": TODAY,
             "note": "18 TLS-era globals moved to thread-local cells; data symbols removed; headers use the upstream accessor/macro contract"},
        ],
    },
    {
        "id": "R-000191",
        "status": "FIXED",
        "phase": "13",
        "title": "HOSTILE-ABI: buffer/limits contracts diverged under extreme sizes and NULLs (xmlReadMemory INT_MAX wild-read, xmlBufferCreate/CreateSize negative/overflow, xmlParseChunk NULL/negative, buf_add/buf_add_head edge contract)",
        "surface": "Parser entry points and buffer primitives under hostile arguments",
        "component": [
            "src/abi/exports_xml2.rs",
            "src/abi/exports_parser.rs",
            "src/abi/exports_buffer.rs",
        ],
        "discovery_date": TODAY,
        "fixed_date": TODAY,
        "root_cause": "xmlReadMemory accepted size >= INT_MAX and streamed ~2 GiB from the caller's buffer (upstream's own unsized wild read); xmlBufferCreate/CreateSize mishandled negative/overflow sizes; xmlParseChunk did not reject NULL/negative chunks with XML_ERR_ARGUMENT; buf_add/buf_add_head did not reproduce the upstream edge contract (NULL -> -1, negative len -> strlen, 0 -> 0, 0x80000000 cap).",
        "fix": "xmlReadMemory now rejects size == INT_MAX up front (observable result matches the oracle's deterministic probe outcome); xmlBufferCreate/CreateSize follow the upstream contract (xml_buffer_create_upstream); xmlParseChunk NULL/negative -> XML_ERR_ARGUMENT; buf_add/buf_add_head implement the upstream edge contract exactly.",
        "observable_residual": "None on the executed platform: 72 NULL/extreme-size attacks byte-identical vs the oracle.",
        "triangulation": "HOSTILE-ABI probe (72 attacks) byte-identical; security-limits probe unchanged.",
        "regression_courts": ["HOSTILE-ABI"],
        "evidence": ["courts/receipts/phase-13/hostile-abi-*.json"],
        "classification": "CANDIDATE_BUG",
        "history": [
            {"status": "OPEN", "date": TODAY,
             "note": "HOSTILE-ABI probe exposed the buffer/limits divergences"},
            {"status": "FIXED", "date": TODAY,
             "note": "upstream-faithful buffer contracts + INT_MAX rejection"},
        ],
    },
    {
        "id": "R-000192",
        "status": "FIXED",
        "phase": "13",
        "title": "HOSTILE-OWNERSHIP: NULL-handling divergences (xmlNewNode/xmlNewPI accepted NULL names, create_int_subset NULL-doc, utf8_strlen(NULL))",
        "surface": "Tree construction and string helpers under NULL attacks",
        "component": [
            "src/xml/tree/mod.rs",
            "src/xml/dtd/mod.rs",
            "src/xml/string.rs",
        ],
        "discovery_date": TODAY,
        "fixed_date": TODAY,
        "root_cause": "xmlNewNode/xmlNewPI did not reject NULL names like upstream; create_int_subset with a NULL document created an attached DTD instead of an unattached one; utf8_strlen(NULL) did not return -1.",
        "fix": "NULL names rejected with upstream messages; create_int_subset(NULL, ...) allocates an unattached DTD exactly like upstream; utf8_strlen(NULL) -> -1.",
        "observable_residual": "None: HOSTILE-OWNERSHIP O1-O12 byte-identical.",
        "triangulation": "HOSTILE-OWNERSHIP probe (O1-O12) byte-identical.",
        "regression_courts": ["HOSTILE-OWNERSHIP"],
        "evidence": ["courts/receipts/phase-13/hostile-ownership-*.json"],
        "classification": "CANDIDATE_BUG",
        "history": [
            {"status": "OPEN", "date": TODAY,
             "note": "HOSTILE-OWNERSHIP probe exposed the NULL-handling divergences"},
            {"status": "FIXED", "date": TODAY,
             "note": "upstream NULL contracts implemented"},
        ],
    },
    {
        "id": "R-000193",
        "status": "FIXED",
        "phase": "13",
        "title": "HOSTILE-ALLOCATOR: xmlStrcat/xmlStrncat leaked the old buffer on realloc failure; xml_buf_add did not reproduce the upstream failure contract",
        "surface": "String/buffer append paths under allocator-failure injection",
        "component": [
            "src/xml/string.rs",
            "src/abi/exports_buffer.rs",
        ],
        "discovery_date": TODAY,
        "fixed_date": TODAY,
        "root_cause": "xmlStrcat/xmlStrncat did not free the previous buffer when the reallocation failed (upstream xmlstring.c frees it and returns NULL), leaking under failure injection; xml_buf_add returned the wrong result on failure.",
        "fix": "xmlStrcat/xmlStrncat free `cur` on realloc failure and return NULL; xml_buf_add implements the upstream contract (0 on success, -1 on failure, no partial write).",
        "observable_residual": "None: HOSTILE-ALLOCATOR H1-H6 byte-identical under size-based failure injection.",
        "triangulation": "HOSTILE-ALLOCATOR probe (H1-H6) byte-identical; allocator-hook and allocator-default courts unchanged.",
        "regression_courts": ["HOSTILE-ALLOCATOR", "ALLOCATOR-HOOK"],
        "evidence": ["courts/receipts/phase-13/hostile-allocator-*.json"],
        "classification": "CANDIDATE_BUG",
        "history": [
            {"status": "OPEN", "date": TODAY,
             "note": "HOSTILE-ALLOCATOR probe exposed the realloc-failure leak"},
            {"status": "FIXED", "date": TODAY,
             "note": "upstream xmlstring.c failure contract implemented"},
        ],
    },
    {
        "id": "R-000194",
        "status": "FIXED",
        "phase": "13",
        "title": "HOSTILE-CALLBACKS: xmlSAXUserParseMemory/File freed the caller's SAX handler (stack ownership), returned -1 instead of errNo, lost the error-context parent input, and did not propagate I/O source failures",
        "surface": "SAX callback lifecycle and error-context plumbing under hostile callbacks",
        "component": [
            "src/abi/exports_parser.rs",
            "src/xml/parser/state.rs",
            "src/xml/parser/input.rs",
            "src/xml/errors/mod.rs",
        ],
        "discovery_date": TODAY,
        "fixed_date": TODAY,
        "root_cause": "xmlSAXUserParseMemory/xmlSAXUserParseFile copied the caller's SAX into the parser context and then freed it at teardown (freeing a stack object or the caller's storage), returned -1 instead of the raised error number, resolved the error context against the wrong input when the user SAX raised during a nested input, and swallowed read-callback failures (reporting an empty document instead of the I/O error).",
        "fix": "The user-SAX wrappers deep-copy the handler into library-owned storage and free only that copy; they return errNo; the error context falls back to the parent input; I/O source failures raise XML_IO_UNKNOWN and the 'Document is empty' path is only taken when the source genuinely produced no bytes.",
        "observable_residual": "None: HOSTILE-CALLBACKS C1-C10 byte-identical.",
        "triangulation": "HOSTILE-CALLBACKS probe (C1-C10) byte-identical.",
        "regression_courts": ["HOSTILE-CALLBACKS", "CALLBACK-001"],
        "evidence": ["courts/receipts/phase-13/hostile-callbacks-*.json"],
        "classification": "CANDIDATE_BUG",
        "history": [
            {"status": "OPEN", "date": TODAY,
             "note": "HOSTILE-CALLBACKS probe exposed the SAX ownership/errNo/error-context/I/O-failure divergences"},
            {"status": "FIXED", "date": TODAY,
             "note": "user-SAX copy ownership, errNo return, parent-input error context, I/O failure propagation implemented"},
        ],
    },
    {
        "id": "R-000195",
        "status": "FIXED",
        "phase": "13",
        "title": "HOSTILE-FAILURE: diagnostic-surface divergences (regexp handle typedefs missing, depth-limit error not streamed, entity-loop legacy 'cur input' tail, XPath compile diagnostics, xmlParseDTD I/O warning, xmlRegexpCompile invalid patterns)",
        "surface": "Error/diagnostic output for hostile documents and malformed inputs",
        "component": [
            "include/libxml/xmlregexp.h",
            "src/xml/parser/tokenizer.rs",
            "src/xml/errors/mod.rs",
            "src/xml/xpath/",
            "src/xml/regex/mod.rs",
            "src/abi/exports_xml2.rs",
        ],
        "discovery_date": TODAY,
        "fixed_date": TODAY,
        "root_cause": "The drop-in headers did not declare the xmlRegexpPtr/xmlRegExecCtxtPtr handles (F8); the depth-limit error was raised without the source window (F1); the entity-loop error lacked upstream's legacy 'cur input' tail line (F2); xmlXPathCompile produced no diagnostics with byte offsets (F3); xmlParseDTD did not emit the file-load I/O warning (F7); xmlRegexpCompile returned a compiled object for invalid patterns instead of NULL (F8).",
        "fix": "Declared the regexp handle typedefs verbatim from the oracle header; factorized window_at_data and streamed the depth error with a window; plumbed the legacy tail through raise_error_streamed/format_error_streamed; added compile_result + byte offsets to the XPath lexer diagnostics; made xmlParseDTD emit the I/O warning; xmlRegexpCompile returns NULL when the NFA cannot be built.",
        "observable_residual": "None: HOSTILE-FAILURE F1-F10 byte-identical, including the legacy tail line.",
        "triangulation": "HOSTILE-FAILURE probe (F1-F10) byte-identical; HEADER-COMPILE 596/596.",
        "regression_courts": ["HOSTILE-FAILURE", "HEADER-COMPILE"],
        "evidence": ["courts/receipts/phase-13/hostile-failure-*.json"],
        "classification": "CANDIDATE_BUG",
        "history": [
            {"status": "OPEN", "date": TODAY,
             "note": "HOSTILE-FAILURE probe exposed the diagnostic-surface divergences"},
            {"status": "FIXED", "date": TODAY,
             "note": "regexp typedefs, streamed windows, legacy tail, XPath diagnostics, DTD I/O warning, invalid-regexp NULL implemented"},
        ],
    },
    {
        "id": "R-000196",
        "status": "FIXED",
        "phase": "13",
        "title": "format-number(-inf) emitted heap garbage: xml_strdup_joined was called on a NON-NUL-terminated '-Infinity' buffer (read-past-end until a NUL), exposed by the Phase-13 TLS data-segment layout shift (CLI-XSLTPROC-0017)",
        "surface": "xsltFormatNumberConversion negative-infinity path (numbering/mod.rs)",
        "component": [
            "src/xslt/numbering/mod.rs",
        ],
        "discovery_date": TODAY,
        "fixed_date": TODAY,
        "root_cause": "The -Infinity branch assembled `joined = minusSign + infinity` as a Vec WITHOUT a NUL terminator and passed it to xml_strdup_joined, which measures the input with strlen (xml_strdup) — so the copy read past the Vec's end until an arbitrary NUL, emitting heap garbage (6 bytes EF BF BD 74 71 7F in the CLI-XSLTPROC-0017 heap layout). The bug predates Phase 13 (present at the Phase-12 seal) and was hidden by the previous heap layout; the Phase-13 TLS conversion (18 data symbols removed) shifted the layout and made it observable.",
        "fix": "NUL-terminate the joined buffer (joined.push(0)) before xml_strdup_joined; the positive-infinity/NaN branches already point at NUL-terminated statics.",
        "observable_residual": "None: CLI-XSLTPROC-0017 and the full xsltproc court (21/21) are byte-identical again.",
        "triangulation": "xsltproc CLI court 21/21; minimal repro (fmtmin3.xsl) byte-identical; the failing heap layout was reproduced before the fix and eliminated after it.",
        "regression_courts": [
            "CLI-XSLTPROC",
        ],
        "evidence": [
            "courts/receipts/phase-09/xsltproc-*.json",
        ],
        "classification": "CANDIDATE_BUG",
        "history": [
            {"status": "OPEN", "date": TODAY,
             "note": "CLI-XSLTPROC-0017 failed after the Phase-13 TLS data-segment shift: '-Infinity' followed by 6 garbage bytes"},
            {"status": "FIXED", "date": TODAY,
             "note": "joined buffer NUL-terminated before xml_strdup_joined"},
        ],
    },
]


def main():
    with open(LEDGER) as f:
        ledger = json.load(f)
    known = {r["id"] for r in ledger["ledger"]}
    added = 0
    for entry in NEW:
        if entry["id"] in known:
            print(f"skip {entry['id']} (already present)")
            continue
        ledger["ledger"].append(entry)
        added += 1
    if "13" not in ledger["phases"]:
        ledger["phases"].append("13")
    with open(LEDGER, "w") as f:
        json.dump(ledger, f, indent=1)
        f.write("\n")
    print(f"added {added} residuals; phases now include 13")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
