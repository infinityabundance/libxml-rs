# Phase 14.23 — xsl xinclude (document() + doXInclude): 3 -> 2

Gate: xpe-six40.log = 2 failures (xmlreader fromStream_broken_stream |
xmlwriter shiftjis = oracle-failing parity). NEW_ONLY empty vs six39.
cargo fmt clean, cargo test --lib 1241 pass. Valgrind-clean.

doXInclude (XSLTProcessor::$doXInclude sets ctxt->xinclude directly) is now
honored by xsltLoadDocument: a document loaded through the document()
function is XInclude-processed with the context's parser options BEFORE it
is cached (documents.c xsltLoadDocument parity).

The heap crash (malloc_consolidate / "invalid chunk size" under the run-tests
harness and valgrind "Invalid free") was a DOUBLE FREE in the xinclude
engine: process_single_include freed the `parse` attribute string inside the
parse-mode scan and AGAIN in the tail cleanup (the phpt's data.xml carries
parse="xml"). parse_attr is now nulled after the first free
(src/xml/xinclude/mod.rs).

Remaining candidate-driven: xmlreader fromStream_broken_stream (the recorded
incremental/pull-stream reader architectural item). xmlwriter shiftjis is
oracle-failing parity (its .exp is unsatisfiable — raw Shift_JIS bytes can
never equal the checked-in empty-comment text; the oracle fails it too).

xsl family: ZERO failures.
