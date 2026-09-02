# Phase 14.3 — Candidate crash-class reproduction (diagnostic)

Status: Bug-1 FIXED. Other crash-class members remain OPEN (root cause per member).

## Discovery
The Phase-14.3 fail-closed result interpreter (`consumers/php-court-result.py`)
classifies candidate failures by native content. Of the 321 current-HEAD
failures, 31 carry an unambiguous OS/allocator fault signature rather than
an ordinary output mismatch. Listing committed alongside the re-baseline:
`courts/receipts/phase-14/php-14-3-baseline-20260902/candidate-crash-list.txt`.

## Representative bug 1 — legacy DOM fragment serialize use-after-free

PHPT: `ext/dom/tests/DOMParentNode_empty_argument.phpt`

Reduced candidate-only repro (`probe_e3` equivalent), deterministic (15/15 fail):

    $dom = new DOMDocument();
    $dom->loadXML("<root><node/></root>");
    $ef = $dom->createDocumentFragment();
    $dom->documentElement->after(...$ef->childNodes);   // empty -> no-op
    $dom->documentElement->firstChild->replaceWith(...$ef->childNodes);
    $dom->documentElement->replaceWith(...$ef->childNodes); // root removed
    $fragment = $dom->createDocumentFragment();
    $fragment->append($dom->createElement('foo'));
    $fragment->append(...$ef->childNodes);
    $fragment->prepend(...$ef->childNodes);
    $fragment->append(); $fragment->prepend();
    echo $dom->saveXML($fragment);          // <-- SIGSEGV (PHP Zend _efree)

Oracle control: identical body runs rc 0 with no signal.

Native backtrace (candidate, Zend allocator on):
    SIGSEGV  _efree -> rc_dtor_func -> ZEND_ECHO_SPEC_TMPVAR_HANDLER
i.e. the returned `saveXML($fragment)` temporary zend_string is released and
crashes => stale buffer pointer.

Decisive allocator facts:
- `USE_ZEND_ALLOC=0` (system malloc) => the body RUNS CLEAN (rc 0). So it is a
  genuine latent use-after-free that Zend's reuse-after-free allocator turns
  into a crash. glibc `MALLOC_CHECK_=3 / MALLOC_PERTURB_=165` do not fault
  under system malloc either => the stale region is read/freed by the Zend
  allocator specifically, i.e. PHP/Zend outlives a native object that the
  candidate released while a PHP proxy/string still references it.
- Therefore this is a libxml-rs ownership/lifetime divergence in the serialize
  tree, not a plain double-free inside one libxml call.

## Bug-1 MINIMAL repro + decisive root-cause narrowing

    $dom = new DOMDocument(); $dom->loadXML("<root/>");
    $f = $dom->createDocumentFragment();
    $f->append($dom->createElement("foo"));
    $out = $dom->saveXML($f);   // candidate: empty output + double free

Oracle: `saveXML($frag)` == "<foo/>"  (element-only save also "<foo/>").
Candidate (plain build, Zend allocator): SIGSEGV in _efree.

The same code run with `USE_ZEND_ALLOC=0` (system malloc) does NOT fault
(latent UAF). Building the candidate PHP with `--enable-debug` turns it into a
decisive assertion:

    Zend/zend_variables.c:64: zend_string_destroy:
      Assertion `!(zval_gc_flags((str)->gc.u.type_info) & (1<<6))' failed.

=> the zend_string returned by `saveXML($fragment)` is DOUBLE-DESTROYED, and
(separately) the fragment serialization produced EMPTY output on the candidate.
So two candidate defects in one path:
  1. serializing a DOMDocumentFragment emits no bytes (direct `out` writes are
     used for element/fragment children, whereas an element-alone save works,
     and document saves go through xmlSaveDoc -> xmlSaveFlush on ctxt);
  2. the returned output zend_string is destroyed more than once.

Raw-I/O differential probe (xmlOutputBufferCreateIO + write + flush + close)
matches oracle byte-for-byte on the candidate, so the plain IO pipe is NOT the
culprit. The divergence sits in the fragment-vs-element serialize subtree that
PHP drives (ext/dom document.c dom_document_save_xml -> handlers->dump_node_to_str
-> php_new_dom_dump_node_to_str_ex -> dom_xml_serialize iterating node->children
for XML_DOCUMENT_FRAG_NODE, element written via ctx->out).

## Hypothesized mechanism (ext/dom forensics so far)
`DOMDocument::saveXML(node)` -> `document.handlers->dump_node_to_str` ->
`php_new_dom_dump_node_to_str_ex` builds a `smart_str`, opens
`xmlSaveToIO(...,&str)` and an extra `xmlOutputBufferCreateIO(...,&str)`, runs
`dom_xml_serialize` (the spec serializer). For an XML_DOCUMENT_FRAG_NODE it
iterates `node->children` and serializes each element through `ctx->out`; a
bare `saveXML(element)` serializes the same element and works, so the delta is
in how the fragment container path drives the output/save context (and in the
candidate it both yields nothing and drops/loses ownership so the returned
string is later double-destroyed). Next step is a Rust/code instrumented pass
(or --enable-debug PHP already built) over php_new_dom_dump_node_to_str_ex to
see which candidate output/save call abandons the emitted bytes.

## Cross-cutting crash-class members (likely shared/adjacent roots)
- ext/simplexml/007, ext/xsl/importStylesheet_clone_retained_{document,node},
  DOMDocument_saveXML_XML_SAVE_NO_DECL, DOMNode_isEqualNode, etc.: double
  free / SIGSEGV during fragment / document serialize or teardown.

## Strict rules honoured while investigating
- No PHP patch, no .phpt change, no expected-output normalisation.
- Candidate crashes are first-class parity failures (14.3-O).

## Next actions (in order)
1. **FIXED (see below)** Attribute the empty-output + double-destroy to the
   exact candidate output path.
2. **FIXED** Add Rust regression court that serializes a document fragment to
   bytes and asserts non-empty.
3. **FIXED** Fix libxml-rs generically.
4. Repeat for the remaining 30 crash-class members; each is attributed,
   courted, full-suite re-verified, and residualized.
5. Update atlas + receipts per fix.

## RESOLUTION — Bug-1 (DOM fragment serialization) FIXED

Root cause: `node_dump_internal` (`src/xml/tree/mod.rs`) had **no
XML_DOCUMENT_FRAG_NODE arm**; a fragment node hit the `_ => {}` empty arm and
dumped zero bytes. Upstream `xmlsave.c` 2.15 trampolines a fragment's children
(transparent container). The empty output is what made `DomDocument pass`
legacy `saveXML(fragment)` return empty, and the downstream PHP serializer then
double-destroyed the returned zend_string (ZEND_DEBUG assertion).

Generic fix: add the `XML_DOCUMENT_FRAG_NODE` arm that iterates `cur->children`
and dumps each as its own root.

Proof:
- Rust regression `xml::tree::tests::test_dump_document_fragment_serializes_children`
  PASSES.
- C oracle differential `consumers/fragdump_probe.c`: oracle and candidate both
  print `FRAG=[<foo/>] ELM=[<foo/>]` (byte-identical).
- PHPT `ext/dom/tests/DOMParentNode_empty_argument.phpt` now PASSES on candidate.
- Full candidate suite 321 -> 320 failures, zero regressions (only that PHPT
  left the fail set).
- No PHP source / no .phpt / no expected-output changes.

Remaining crash-class members (now ~30) are separate roots to be driven to zero
one per residual.
