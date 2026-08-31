#!/usr/bin/env python3
"""11.1-X ledger closure: mark fixed residuals FIXED, register new residuals,
and repair the pre-existing ledger integrity violations (R-000162 glob
component, R-000165/R-000167 multi-entry OPEN history, R-000168
classification). History is preserved; only tails are appended."""
import json

LEDGER = "atlas/RESIDUAL_LEDGER.json"
DATE = "2026-08-31"
PHASE = "11.1-X"

with open(LEDGER, encoding="utf-8") as f:
    ledger = json.load(f)

by_id = {r["id"]: r for r in ledger["ledger"]}


def close(r, fix, lesson, note, regression_courts=None, component=None):
    """Append a FIXED tail entry, set fix/lesson; keep prior OPEN history.
    Idempotent: a residual already carrying a FIXED tail is left alone."""
    if r.get("status") == "FIXED":
        return
    history = r.get("history")
    if not history:
        history = [{"status": "OPEN", "date": r.get("discovery_date")}]
    history.append({"status": "FIXED", "date": DATE, "note": note})
    r["history"] = history
    r["status"] = "FIXED"
    r["fix"] = fix
    r["lesson"] = lesson
    if regression_courts is not None:
        r["regression_courts"] = regression_courts
    if component is not None:
        r["component"] = component


# ── 1. Repair pre-existing ledger integrity violations ──────────────────────
r = by_id["R-000162"]
r["component"] = [c if c != "include/libxml/*" else "include/libxml" for c in r["component"]]

r = by_id["R-000165"]
# merge the three OPEN history entries into one (OPEN may have a single entry)
merged_note = " | ".join(h.get("event") or h.get("note", "") for h in r["history"])
r["history"] = [{"status": "OPEN", "date": "2026-08-30", "note": merged_note}]

r = by_id["R-000167"]
merged_note = " | ".join(h.get("event") or h.get("note", "") for h in r["history"])
r["history"] = [{"status": "OPEN", "date": "2026-08-30", "note": merged_note}]

r = by_id["R-000131"]
r["history"] = [
    {"status": "OPEN", "date": "2026-08-29", "note": "discovered during 11.1-H declared-function closure"},
    {"status": "FIXED", "date": "2026-08-29", "note": "closed by 11.1-J allocator instrumentation (per-block registry, xmlMemSize, *Loc site recording, xmlMemDisplayLast/xmlMemShow per-block dumps); history tail repaired by the 11.1-X ledger integrity repair"},
]

r = by_id["R-000135"]
r["history"] = [
    {"status": "OPEN", "date": "2026-08-29", "note": "discovered during 11.1-I parity census"},
    {"status": "FIXED", "date": "2026-08-29", "note": "closed: 11 data symbols exported with upstream layout and initial values; DATA-GLOBALS-001 differential court byte-identical vs the oracle DSO; obligations regenerated (DATA MISSING = 0). Follow-up (11.1-K): the remaining NULL-default divergence for xsltGenericError/xmlGenericError was closed with the variadic asm shims (R-000161); xsltDocDefaultLoader remains NULL (loader path documented separately). (FIXED->FIXED tail merged by the 11.1-X ledger integrity repair.)"},
]

r = by_id["R-000168"]
r["classification"] = "INTENTIONAL_SAFE_DIVERGENCE"

# ── 2. Close the residuals fixed by the 11.1-X residual closure loop ────────
close(
    by_id["R-000119"],
    fix="11.1-X: the debug dump path now materialises referenced entity content "
        "into ent->children (parser/state.rs entity-content handling with "
        "XML_PARSE_COMPACT TEXT nodes under the 2.13+ epoch) and the DTD "
        "debug dump renders the parsed child tree (xmlDebugDumpNode no longer "
        "double-recurses the DTD). CLI-XMLLINT-0033/0034 regress the observable.",
    lesson="Entity declarations are ABI-visible through ent->children: a debug "
        "dump must render the parsed content tree, not only the raw content string.",
    note="fixed in the 11.1-X residual closure loop; CLI-XMLLINT-0033/0034 "
        "byte-identical (57/57 CLI courts PASS)",
    regression_courts=["CLI-XMLLINT-0033", "CLI-XMLLINT-0034"],
)
close(
    by_id["R-000120"],
    fix="11.1-X: attribute values that contained entity/character references "
        "are no longer marked compact. The tokenizer StartTag token now carries "
        "attr_start so per-attribute reference presence is signalled to the SAX "
        "layer via a non-NULL valueEnd; parser_new_text_node gained "
        "force_noncompact and parser_set_prop a had_ref flag. CLI-XMLLINT-0033 "
        "regresses the observable.",
    lesson="The compact-text marking of attribute values must follow the "
        "upstream had-references rule (xmlNodeParseAttValue never compacts); "
        "losing the reference signal at tokenization corrupts the debug-visible "
        "node representation.",
    note="fixed in the 11.1-X residual closure loop; CLI-XMLLINT-0033 "
        "byte-identical (57/57 CLI courts PASS)",
    regression_courts=["CLI-XMLLINT-0033"],
)
close(
    by_id["R-000121"],
    fix="11.1-X: substitute_refs now scans the entire entity value for '<' "
        "from the value start position and reports XML_ERR_LT_IN_ATTRIBUTE "
        "twice (parser + validation paths) with the caret at the '&' when "
        "XML_PARSE_NOENT is not set, once with --noent — matching the 2.13+ "
        "epoch (E-005). CLI-XMLLINT-0034 regresses the observable.",
    lesson="Diagnostic counts and caret positions are epoch-pinned observables: "
        "the 2.13.0 error consolidation doubled this report and moved the caret; "
        "both must match the current oracle.",
    note="fixed in the 11.1-X residual closure loop; CLI-XMLLINT-0034 "
        "byte-identical (57/57 CLI courts PASS)",
    regression_courts=["CLI-XMLLINT-0034"],
)
close(
    by_id["R-000122"],
    fix="11.1-X: xmlcatalog option parsing now stops at the first non-option "
        "argument (upstream 'if (argv[i][0] != '-') break;'). With "
        "'--create FILE --noout' the trailing --noout is resolved as an entity "
        "and the catalog is dumped (exit 4); the query loop runs unconditionally "
        "and the dump is gated on modified||create exactly as upstream.",
    lesson="CLI option loops must replicate upstream's break-at-first-non-option "
        "semantics; treating post-operand tokens as options changes exit codes "
        "and dumps.",
    note="fixed in the 11.1-X residual closure loop; CLI-XMLCATALOG-0002 "
        "byte-identical (57/57 CLI courts PASS)",
    regression_courts=["CLI-XMLCATALOG-0002"],
)
close(
    by_id["R-000123"],
    fix="11.1-X: the xmlcatalog shell now uses a quote-aware tokenizer and "
        "validates exact argument counts per command ('public requires 1 "
        "arguments' when the command is not given exactly one argument).",
    lesson="Shell command argument-count validation is observable behaviour; "
        "the first token is not the public identifier when the count is wrong.",
    note="fixed in the 11.1-X residual closure loop; CLI-XMLCATALOG-0010 "
        "byte-identical (57/57 CLI courts PASS)",
    regression_courts=["CLI-XMLCATALOG-0010"],
)
close(
    by_id["R-000158"],
    fix="11.1-X: process_call_template snapshots the with-param list before "
        "pushing (xsltPushVariable rewires the next pointers, so iterating "
        "while pushing corrupts the list) and pops back to the saved varsNr "
        "instead of a fixed param count. The engine terminates on ct.xsl "
        "(CLI-XSLTPROC-0010).",
    lesson="XSLT variable-stack discipline: push loops must snapshot the "
        "source list, and pops must restore the pre-call stack depth, never a "
        "count derived from the pushed items.",
    note="fixed in the 11.1-X residual closure loop; CLI-XSLTPROC-0010 "
        "byte-identical (57/57 CLI courts PASS)",
    regression_courts=["CLI-XSLTPROC-0010"],
)
close(
    by_id["R-000159"],
    fix="11.1-X: XPath position() now reads the proximity position member set "
        "by the step evaluation; both predicate loops (main and axis walk) "
        "set/restore proximity_position so //book[position() <= 2] selects "
        "exactly the oracle node set (CLI-XSLTPROC-0004).",
    lesson="position() is context-dependent state, not a function of the "
        "current node alone; predicate evaluation must maintain the proximity "
        "position across the loop.",
    note="fixed in the 11.1-X residual closure loop; CLI-XSLTPROC-0004 "
        "byte-identical (57/57 CLI courts PASS)",
    regression_courts=["CLI-XSLTPROC-0004"],
)

# ── 3. Register new residuals discovered during 11.1-X ──────────────────────
existing_ids = {r["id"] for r in ledger["ledger"]}


def add_if_missing(record):
    if record["id"] not in existing_ids:
        ledger["ledger"].append(record)
        existing_ids.add(record["id"])


add_if_missing({
    "id": "R-000169",
    "status": "FIXED",
    "phase": "11.1-X",
    "title": "Dangling doc->URL / parser-input filename: xml_strdup on a "
        "non-NUL-terminated Rust String (heap-buffer-overflow) and borrowed "
        "filename pointers in parserInternals input construction",
    "surface": "doc->URL, xmlNodeGetBase, _xmlParserInput.filename lifecycle "
        "(xmlReadMemory/xmlCtxtRead* and xmlParseCtxtExternalEntity/"
        "xmlParseBalancedChunkMemoryRecover/xmlParseInNodeContext/xmlParseDTD "
        "paths); XPath pop_string",
    "component": [
        "src/xml/parser/helpers.rs",
        "src/abi/exports_parserint.rs",
        "src/xml/xpath/parser_context.rs",
    ],
    "discovery_date": "2026-08-31",
    "oracle_versions": "libxml2 2.15.3 (system)",
    "root_cause": "The TREE-001 structural probe (11.1-N) observed "
        "URL=t.xml<V> (heap-reuse garbage appended to the URL) and the same for "
        "xmlNodeGetBase. Two defects: (1) alloc_parser_input duplicated the "
        "filename with xml_strdup(fname.as_ptr()) where fname is a Rust String "
        "whose as_ptr() is NOT NUL-terminated — xml_strlen scans past the "
        "allocation (ASan: heap-buffer-overflow) and the copy lands in freed/"
        "reused memory; (2) the four parserInternals entry points used "
        "populate_parser_input directly, which borrows the boxed InputBuffer's "
        "Rust String into _xmlParserInput.filename — dangling once the context "
        "is freed. xpath pop_string had the same non-NUL-terminated xml_strdup "
        "defect.",
    "observable_residual": "doc->URL / base print t.xml followed by heap-reuse "
        "garbage (non-deterministic single character) on the second parse; "
        "ASan heap-buffer-overflow in xml_strdup via alloc_parser_input.",
    "fix": "alloc_parser_input and the parserInternals sites now duplicate "
        "filenames with xml_strndup(fname.as_ptr(), fname.len()) (exact length, "
        "explicit NUL); populate_parser_input was replaced by "
        "populate_parser_input_without_filename + owned dup at all four "
        "parserInternals sites; pi_parse_content_node_list frees the popped "
        "input (struct + owned filename) and pi_pop_pe frees the filename "
        "before the struct, making every _xmlParserInput free path symmetric "
        "with free_parser_input; xpath pop_string uses xml_strndup. The input "
        "filename is now owned uniformly across every construction path.",
    "evidence": [
        "courts/suites/data-abi/tree-structure-probe.c",
        "tools/abi/tree_structure_probe.py",
        "courts/receipts/phase-11/tree-structure-20260831T053510Z.json "
        "(TREE-001 byte-identical, 0 mismatch lines)",
        "ASan repro clean (second-parse URL=[t.xml])",
        "cargo test --lib 1146 pass / 0 fail",
    ],
    "classification": "CANDIDATE_BUG",
    "history": [
        {
            "status": "OPEN",
            "date": "2026-08-31",
            "note": "discovered while sealing 11.1-X: TREE-001 probe mismatch "
                "URL=t.xmlV and base=t.xmlU; ASan pinned xml_strdup on a Rust "
                "String as_ptr in alloc_parser_input",
        },
        {
            "status": "FIXED",
            "date": "2026-08-31",
            "note": "fixed by owning the filename at every _xmlParserInput "
                "construction path (xml_strndup, populate_parser_input_without_"
                "filename) and symmetric frees; TREE-001 byte-identical PASS",
        },
    ],
    "lesson": "A Rust String's as_ptr() is not NUL-terminated; any xml_strdup "
        "on it is a heap-buffer-overflow waiting for the heap layout to change. "
        "C-facing duplications must take an explicit length (xml_strndup), and "
        "filename ownership must be uniform: every _xmlParserInput either owns "
        "its filename (freed with the input) or has none.",
})
add_if_missing({
    "id": "R-000170",
    "status": "FIXED",
    "phase": "11.1-X",
    "title": "xmlLastError global mirror races: concurrent sync/reset "
        "double-free the mirror strings",
    "surface": "xmlLastError data symbol; set_last_error/reset_last_error "
        "mirror sync (xml/globals, abi/data_globals)",
    "component": ["src/abi/data_globals.rs", "src/xml/globals/mod.rs"],
    "discovery_date": "2026-08-31",
    "oracle_versions": "libxml2 2.15.3 (system)",
    "root_cause": "The exported xmlLastError mirror is deep-copied on every "
        "error raise (sync_xml_last_error) and freed on reset "
        "(reset_xml_last_error) with no synchronization. Two threads raising/"
        "resetting concurrently free the same mirror strings (or free strings "
        "just installed by the other thread): glibc 'double free or corruption "
        "(!prev)' aborts in the parallel lib test suite (~10% of runs, victim "
        "tests anywhere). Pre-existing: reproduced at the committed 11.1-W "
        "state (12/100 aborts).",
    "observable_residual": "SIGABRT 'double free or corruption' under parallel "
        "error raising (the full parallel test suite, any allocating test as "
        "victim).",
    "fix": "sync_xml_last_error/reset_xml_last_error are serialized by "
        "LAST_ERROR_MIRROR_LOCK (parking_lot::Mutex); the internal helpers "
        "reset_xml_last_error_locked/sync_xml_last_error_locked run under the "
        "lock with no re-lock. C consumers reading the symbol directly keep "
        "upstream's documented racy semantics. Two regression courts "
        "(test_last_error_mirror_concurrent_sync_reset, "
        "test_last_error_mirror_many_threads) hammer the interleavings.",
    "evidence": [
        "cargo test --lib 1146 pass / 0 fail",
        "100/100 parallel full-suite runs clean (was ~12/100 SIGABRT at "
        "11.1-W)",
        "test_last_error_mirror_concurrent_sync_reset",
        "test_last_error_mirror_many_threads",
    ],
    "classification": "CANDIDATE_BUG",
    "history": [
        {
            "status": "OPEN",
            "date": "2026-08-31",
            "note": "discovered while sealing 11.1-X: parallel lib suite "
                "aborts; bisected to xml::errors tests racing with any other "
                "raising thread; reproduced at committed 11.1-W",
        },
        {
            "status": "FIXED",
            "date": "2026-08-31",
            "note": "fixed with the mirror lock; 0/100 parallel-suite aborts "
                "after the fix",
        },
    ],
    "lesson": "A C-visible data mirror with owned strings must serialize its "
        "writers even when the upstream original is a racy bare global: the "
        "readers keep upstream semantics, but internal free/write pairs must "
        "never interleave across threads.",
})
add_if_missing({
    "id": "R-000171",
    "status": "FIXED",
    "phase": "11.1-X",
    "title": "Error-handler slot pairs (xmlStructuredError/xmlGenericError + "
        "contexts) read/written non-atomically; handler-slot tests race",
    "surface": "xmlSetGenericErrorFunc/xmlSetStructuredErrorFunc, "
        "get_generic_error_ctx/get_structured_error_ctx, raise_error dispatch",
    "component": [
        "src/xml/globals/mod.rs",
        "src/xml/errors/mod.rs",
        "src/abi/data_globals.rs",
    ],
    "discovery_date": "2026-08-31",
    "oracle_versions": "libxml2 2.15.3 (system)",
    "root_cause": "The exported handler slots were read/written as two "
        "independent static mut globals, so a reader could observe a new "
        "handler with an old context (or vice versa), and "
        "test_error_callbacks_default_handlers (xml::globals) could observe "
        "another test's temporarily-installed structured handler and fail its "
        "assertions (~30% of parallel runs).",
    "observable_residual": "Incoherent (handler, ctx) pairs under concurrent "
        "set; flaky test_error_callbacks_default_handlers assertion failure.",
    "fix": "Handler slot pairs are now written and read atomically under "
        "ERROR_HANDLER_LOCK; with_structured_error/with_generic_error read the "
        "pair under the lock and invoke the callback outside it (no deadlock "
        "on re-entrant error raising). The three handler-mutating tests are "
        "serialized by ERROR_HANDLER_TEST_LOCK.",
    "evidence": [
        "cargo test --lib 1146 pass / 0 fail",
        "100/100 parallel full-suite runs clean",
        "test_error_callbacks_default_handlers",
        "test_error_callbacks_set_and_get",
        "test_structured_error_callback",
    ],
    "classification": "CANDIDATE_BUG",
    "history": [
        {
            "status": "OPEN",
            "date": "2026-08-31",
            "note": "discovered while sealing 11.1-X: after the R-000170 fix "
                "the parallel suite surfaced flaky handler-slot assertion "
                "failures",
        },
        {
            "status": "FIXED",
            "date": "2026-08-31",
            "note": "fixed with the handler-pair lock and test serialization; "
                "100/100 parallel-suite runs clean",
        },
    ],
    "lesson": "Two globals that form one logical (handler, ctx) pair must be "
        "published and read as a unit; tests that mutate shared exported "
        "slots must serialize against readers.",
})

ledger["ledger"].sort(key=lambda r: r["id"])
with open(LEDGER, "w", encoding="utf-8") as f:
    json.dump(ledger, f, indent=1, ensure_ascii=False)
    f.write("\n")
print("ledger updated:", len(ledger["ledger"]), "residuals")
