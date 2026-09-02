# Phase 14.3 — Candidate crash-class reproduction (diagnostic)

Status: OPEN — not yet root-caused. Recorded so the forensic loop is durable.

## Discovery
The Phase-14.3 fail-closed result interpreter (`consumers/php-court-result.py`)
classifies candidate failures by native content. Of the 321 current-HEAD
failures, 31 carry an unambiguous OS/allocator fault signature rather than
an ordinary output mismatch. Listing committed alongside the re-baseline:
`courts/receipts/phase-14/php-14-3-baseline-20260902/candidate-crash-list.txt`.

## Representative repro (candidate-only)
PHPT: `ext/dom/tests/DOMParentNode_empty_argument.phpt`

Run the PHPT body against the candidate PHP built on libxml-rs DSOs:

    php -n pnfull.php   # SIGSEGV in Zend _efree during
                        # DOMDocumentFragment section, echo $dom->saveXML($fragment)

Oracle (baked libxml2 2.15.3) runs the identical body to rc=0, no signal.

Native backtrace (candidate):
    SIGSEGV  _efree -> rc_dtor_func -> ZEND_ECHO_SPEC_TMPVAR_HANDLER
    So a PHP temporary zend_string returned by the preceding call is freed and
    its buffer pointer is invalid / already freed.

The crash happens after DOMElement section reduced the document to <root/> via
replaceWith(...), then a DOMDocumentFragment with a child is serialized with
$dom->saveXML($fragment). Oracle serializes and returns cleanly.

## Cross-cutting signatures in the same crash class
* ext/simplexml/007:  free(): double free detected in tcache 2 / Aborted
* ext/xsl/importStylesheet_clone_retained_document / ..._node and dom
  saveXML_XML_SAVE_NO_DECL, DOMElement_append_hierarchy_test,
  DOMNode_isEqualNode: Segmentation fault / double free during teardown or
  fragment/document serialization.

Hypothesis to test next (14.3-G document/node ownership + value/lifetime):
ext/dom serializes a DOMDocumentFragment through an xml buffer whose lifetime /
ownership differs on the candidate (e.g. an xmlBuffer the candidate frees once
but PHP also frees, or a document node whose ->doc/->parent retained pointer is
freed while a PHP proxy still references it). Trace ext/dom
DOMDocument::saveXML's node-argument path into the exact libxml call
(xmlDocDump/xmlNodeDump/xmlBufferCreate) and compare oracle ownership.

## Next actions (in order)
1. Attach gdb to php at the _efree crash, print the offending pointer and its
   origin (Zend string buffer vs libxml xmlBuffer).
2. Read ext/dom document.c saveXML + xml_serializer.c to find the libxml call.
3. Write a minimal C oracle/candidate differential probe for the fragment
   serializer ownership.
4. Fix libxml-rs generically + Rust regression test.
5. Re-run representative, then the full crash-class list; re-run full suite.
6. Update atlas receipts.
