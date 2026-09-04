# Phase 14.22 — xsl:copy/xsl:element ns fixup + pattern priorities + URI matching: 4 -> 3

Gate: xpe-six39.log = 3 failures (xsl xinclude | xmlreader broken stream |
xmlwriter shiftjis = oracle-failing parity). NEW_ONLY empty vs six38.
cargo fmt clean, cargo test --lib 1241 pass.

gh21357_2 flipped (the modern-DOM xmlns-pseudo-attribute copy). Root causes
(two layers, both fixed):

A. XSLT result construction was namespace-blind:
   1. process_copy (xsl:copy) of an ATTRIBUTE node copied its VALUE as TEXT
      (transform.c xsltShallowCopyAttr parity: the attribute is copied onto the
      current result element via xsltGetSpecialNamespace + xmlSetNsProp; modern
      DOM xmlns decls stored as attributes in the xmlns namespace reproduce
      literally as ns_1:xmlns="...").
   2. process_copy of an ELEMENT was xsltShallowCopyElem parity: declare the
      source nsDef list on the copy and re-bind the element namespace through
      xsltGetSpecialNamespace (copies lost every in-scope declaration).
   3. process_element (xsl:element): a missing `namespace` attribute now
      resolves the instruction's in-scope default namespace
      (xmlSearchNs(inst->doc, inst, NULL)) and binds via xsltGetSpecialNamespace
      instead of a detached decl (gh21357_2 relies on xmlns= on the
      xsl:element); empty namespace attribute still means no namespace.

B. Pattern matching compared PREFIX STRINGS instead of URIs and had wrong
   default priorities:
   4. xsltCompilePattern now resolves prefixes against the stylesheet doc root
      (NsWildcard->NsWildcardUri, QName->QNameUri); match compares namespace
      URIs, so match="old:*" hits default-namespace and differently-prefixed
      elements (pattern.c xmlGetNsList + xmlSearchNs parity).
   5. Default priorities corrected to upstream pattern.c: QName tests (element/
      attribute/PI-literal) = 0; NCName:* = -0.25; node()/text()/comment()/
      processing-instruction() without literal / * / @* = -0.5. (Old values:
      node() -0.25, @attr +0.5, @* +0.5 made the union node()|@* outrank
      old:*; identity-copy template wrongly beat the namespace template.)
   6. XPath document-order comparator now places an element's attribute nodes
      before its child nodes (xmlXPathCmpNodes parity) — apply-templates
      select="node()|@*" copies attributes onto the still-childless result
      element instead of erroring after children were added.
   7. child axis of an ATTRIBUTE context node is empty (XPath data model) —
      xsl:copy of an attribute no longer re-emits its value text.

8 unit tests updated to the corrected upstream priorities.

Probes: consumers/{xsl-identity,union-order,modern-nsattr}-probe.php.
