# Phase 14.18 — DTD entity `orig` (dom bug67081): 20 -> 18

Gate: xpe-six35.log = 18 failures (dom 9 | xsl 7 | xmlreader 1 | xmlwriter 1).
NEW_ONLY empty vs six34.

Fix: parse_entity_decl (src/xml/parser/state.rs) now records the raw
declaration value text as the entity's `orig` via the new
entities::add_entity_with_orig (src/xml/entities/mod.rs). xmlsave.c parity:
xmlBufDumpEntityDecl prints `orig` verbatim (only `"` escaped) and only the
no-orig fallback escapes `%` -> `&#x25;`. DOMDocumentType->internalSubset
now round-trips `<!ENTITY % attrs "%coreattrs;">` raw (was `&#x25;coreattrs;`).

Also: gh19098 (14.17 reader Next fix) shows flipped in this gate.

# Phase 14.19+ plan — remaining 18 (dom 9 | xsl 7 | xmlreader 1 | xmlwriter 1)

Ranked by fan-in / boundedness:

## A. xsl 7 (biggest remaining block; one new instruction + 3 semantics)
1. bug54446 + bug54446_with_ini (2): register the upstream built-in EXTRA
   extension elements (saxon:output / xalan:write / xt:document /
   xsl:document -> xsltDocumentElem) in xsltRegisterAllExtras and implement
   xsltDocumentElem: AVT file/href -> xmlBuildURI against ctxt->outputFile,
   xsltCheckWrite security (reuse src/xslt/security.rs + xsltCheckWritePath
   semantics: "File write for %s refused"), content evaluated into a
   separate result doc then saved (method=text honors style->method), plus
   the "xsltDocumentElem: write rights for %s denied" message. Per-denied
   transform emits FOUR warnings (context line twice). Verified oracle text
   via /tmp/bug54446o.php on phporacle-c.
2. bug71571_a + bug71571_b (2): maxTemplateDepth / maxTemplateVars with
   Templates:/Variables: dumps and XSLT_STATE_STOPPED; b SEGVs today (no
   varsNr guard). Upstream transform.c xsltApplyXSLTTemplate /
   xsltApplySequenceConstructor guard text captured in .exp (see diffs).
3. req30622 (1): setParameter expanded-QName ({uri}local) storage/lookup;
   remove must match by expanded name too.
4. gh21357_2 (1): xsl:copy/@* of an xmlns-pseudo-namespace attribute
   (modern DOM stores ns decls as attrs in the xmlns URI) -> serializer
   must emit the pseudo attribute literally (ns_1:xmlns="..."), candidate
   currently splats its value as duplicated TEXT.
5. xinclude/xinclude (1): heap crash under the run-tests harness only
   (candidate xinclude of document()); investigate cache/double-free before
   re-enabling (phase 14.16 note).

## B. xmlwriter + dom encoding gap (3): xmlwriter_toStream_encoding_shiftjis,
   XMLDocument_createFrom{String,File}_override_encoding (Windows-1252).
   Requires a real non-native encoding backend (iconv) — W9 R-000157.

## C. dom single-item tails (currently 9: after bug67081, remaining):
   DOMDocument_loadHTMLfile_error1 (I/O warning wording),
   DOMNode_isEqualNode (SEGV — deep-compare crash, likely a specific node
   kind), bug80268_2 (HTML NUL bytes), gh12616_3 (xmlns DOMNameSpaceNode
   removal residue), getLineNo_65536 (HTML line counter cap — line stored
   narrow), serialize_non_default_empty_xmlns (URI check on xmlns:a=" "),
   DTDNamedNodeMap (baseURI of DTD entities = parser CWD; needs
   doc->URL/base propagation on DTD decls), override_encoding pair (B).

## D. xmlreader streaming (1): fromStream_broken_stream — incremental reader
   for pull/php-stream inputs (events delivered before doc completion;
   EOF error only when a later pull needs data; EOF keeps the cursor).
   Reuse probe/partial_delivery pause machinery (SP-14.3.1-6).

Probes/receipts in courts/suites/phase14/consumers/*-probe.php and
courts/receipts/phase-14/php-14-17-*/.
