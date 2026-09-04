# Phase 14.20 — dom big-lines psvi + windows-1252 override converter: 13 -> 10

Gate: xpe-six37.log = 10 failures (dom 6 | xsl 2 | xmlreader 1 | xmlwriter 1).
NEW_ONLY empty vs six36. cargo fmt clean, cargo test --lib 1241 pass.

ORACLE-VERDICT ANALYSIS (phporacle-c run-tests on all 13): 12 of the 13 pass
on the oracle; xmlwriter_toStream_encoding_shiftjis FAILS on the oracle too
(raw Shift_JIS bytes 0x82A1.. can never match the checked-in .exp `<!---->`
which encodes an empty comment — the .exp is unsatisfiable for any correct
SJIS emitter). It is therefore oracle-failing parity, NOT a candidate-driven
failure: candidate-driven target = 9 (dom 6 | xsl 2 | xmlreader 1).

Fixes (3 dom tests flipped):

1. getLineNo_65536 — tree.c xmlGetLineNoInternal parity: text nodes whose
   parse-time line exceeded USHRT_MAX carry the real line in psvi (PHP's
   html5 lexbor bridge stores XML_INT_TO_PTR(line) on text nodes with
   node->line >= USHRT_MAX); the candidate's get_line_no_internal never read
   text psvi, so html/body/p >65535 resolved to the 65535 clamp. Added the
   text-node psvi read (XML_PTR_TO_INT) before the element-children walk
   (src/xml/tree/mod.rs).

2. XMLDocument_createFrom{String,File}_override_encoding — R-000157 partial
   closure: native windows-1252 (CP1252) converter (WHATWG/glibc table incl.
   the five undefined C1 bytes -> EILSEQ-like errors) registered as
   "windows-1252" + "cp1252" handlers with streaming input/output funcs and
   whole-buffer helpers (src/xml/encoding/mod.rs). The override plumbing PHP
   uses (xmlFindCharEncodingHandler -> xmlSwitchToEncoding on a memory parser
   ctxt whose input->buf is NULL) previously failed silently; xmlSwitchToEncoding
   now routes to the stashed Rust InputBuffer (helpers::apply_memory_encoding_override)
   which transcodes the buffered stream and repopulates ctxt->input
   (src/xml/parser/{input,helpers}.rs, src/abi/exports_parser.rs). Decl-time
   doc->encoding stamping is skipped under XML_PARSE_IGNORE_ENC (both in the
   native parse_xml_decl and the SAX endDocument propagation) so PHP fills the
   override name (xml_document.c sets doc->encoding from its argument when the
   parse left it NULL) — upstream xmlSAX2EndDocument semantics.

Oracle verdicts re-confirmed: remaining 9 all PASS on the oracle.

Probes: courts/suites/phase14/consumers/lineno-chain-probe.php (+ sjis-oracle.phpt).
