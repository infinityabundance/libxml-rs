# Phase 14.19 — xsl cluster (params / template guards / EXTRA document write): 18 -> 13

Gate: xpe-six36.log = 13 failures (dom 9 | xsl 2 | xmlreader 1 | xmlwriter 1).
NEW_ONLY empty vs six35. cargo fmt clean, cargo test --lib 1241 pass.

Fixes (5 xsl tests flipped):

1. req30622 — XSLTProcessor::setParameter with expanded-QName ({uri}local):
   params are bound per-transform with expanded names; storage/lookup and
   removeParameter now match by expanded name too. src/xslt/variables/mod.rs
   (xsl-param storage), src/xslt/transform/mod.rs, src/xslt/stylesheet/mod.rs.

2. bug71571_a + bug71571_b — maxTemplateDepth / maxTemplateVars recursion
   guards: xsltApplyXSLTTemplate / xsltApplySequenceConstructor depth and
   varsNr checks, XSLT_STATE_STOPPED, and the Templates:/Variables: debug
   dumps via xsltDebug (NUL-terminated pieces; one slot-head per var frame).
   b SEGV'd before (no varsNr guard).

3. bug54446 + bug54446_with_ini — saxon:output / xalan:write / xt:document
   (and xsl:document) dispatch to the classic EXTRA xsltDocumentElem in
   src/abi/exports_xslt_exec.rs (~line 2159): AVT file/href -> xmlBuildURI
   against ctxt->outputFile, xsltCheckWrite security ("File write for %s
   refused" then "xsltDocumentElem: write rights for %s denied"), content
   evaluated into a separate result doc then saved. Registered via
   xsltRegisterAllExtras; xsltInit + xsltStylesheetCreate call it
   (idempotent into the global EXT_ELEMENTS hash in exports_xslt_ext.rs).

Remaining 13 (see php-14-18 receipt plan sections B/C/D + the two xsl
tails): dom 9 | xsl gh21357_2 + xinclude/xinclude | xmlreader
fromStream_broken_stream | xmlwriter shiftjis.

Probes: courts/suites/phase14/consumers/{depth,param}-probe.php.
