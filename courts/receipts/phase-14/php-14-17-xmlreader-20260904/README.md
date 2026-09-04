# Phase 14.17 — xmlreader schema/cursor/error cluster: 26 -> 20 (xpe-six34.log)

Full-gate logs: xpe-six33.log (21, transient gh22219 regression caught + fixed),
xpe-six34.log = 20 failures, NEW_ONLY empty vs six32.

Split at 20: dom 10 | xmlreader 2 | xmlwriter 1 | xsl 7.

## Root causes fixed (zero-regression in every full gate)

1. RC-1 reader pre-read cursor semantics (fromStream/fromString_custom_constructor)
   - xmlTextReaderAttributeCount: node==NULL -> 0 (was -1 -> php property error);
     END/other non-element positions clamp to 0.
   - xmlTextReaderIsEmptyElement: node==NULL -> -1 (php raises "no XML data...");
     NULL reader also -1.
   - xmlTextReaderConstBaseUri / xmlTextReaderBaseUri: NULL unless a node is
     current (pre-read baseURI was the setup URL = CWD).
   - verified probe rdrprops-probe.php matches oracle field-for-field.

2. RC-2 xmlTextReaderSchemaValidate/RelaxNGValidate deferral + XSD ns engine
   (013, bug73053; plus the gh22219 ns engine regression fixed in-gate)
   - PHP setSchema compiles the schema NOW (parse failure -> -1 -> "Schema
     contains errors") and validates AT READ TIME (upstream SAX-plug model).
     Candidate ran xmlSchemaValidateDoc eagerly and returned its outcome.
     Reader now stores the compiled schema (owned for the file-path API;
     caller-owned for xmlTextReaderSetSchema / RelaxNGSetSchema — php keeps
     that in intern->schema), validates in parse_and_build_events inside the
     first Read(), raises diagnostics through the global error channel
     (errors::raise_error; message must end with '\n' for php's handler),
     records xsd_result/rng_result for xmlTextReaderIsValid (upstream order:
     rng first, then xsd, then DTD).
   - XSD engine matched global root declarations by prefixed-QName STRING.
     bug73053.xsd (targetNamespace urn:books, doc <x:books>) failed
     "No matching global declaration". Now: top-level components are stamped
     with the schema targetNamespace; global lookup + xsd_validate_element
     compare EXPANDED names (local + ns); local declarations match per
     elementFormDefault/`form` (gh22219 elementFormDefault="qualified").
     Named-type base refs compare by local part (bks:BookForm -> BookForm) so
     content models actually validate. nsroot-probe.php == oracle on all 4
     cases; nschild-probe divergence (message text only) documented.
   - xmlTextReaderRelaxNGSetSchema semantic + IsValid for rng/xsd.

3. RC-5 relaxNG include resolution + grammar merge (007)
   - <include href> resolved against the including grammar document's URL
     (xmlBuildURI), not CWD — 007's relaxNG.rng includes relaxNG2.rng.
   - include inline <define> children now REDEFINE the included grammar's
     defines (relaxNG.rng overrides TEI.prose -> ref INCLUDE), and a grammar
     without <start> inherits the included grammar's start pattern.

4. RC-3 bug64230 garbage "Internal: uYUU"
   - xmlCopyError/copy_error was a SHALLOW struct copy. php's libxml error
     list stores xmlCopyError() copies at raise time and reads them later;
     the parser's next raise freed the shared lastError strings (garbage).
     Now deep-copies message/file/str1/str2/str3 exactly like upstream
     error.c xmlCopyError. (Both fns are no longer `const`.)

5. RC-4 gh19098 reader next()/expand() namespace flow
   - xmlTextReaderNext on a fresh (pre-first-Read) reader returned 0;
     upstream degrades to xmlTextReaderRead, i.e. STARTS the traversal.
     php gh19098 opens with next("sparql") before any read.
   - probe == oracle through expand; full phpt byte-identical.

## Deferred (architectural, recorded for 14.19+)
- RC-6 fromStream_broken_stream.phpt: requires an INCREMENTAL reader for
  php-stream inputs (php://memory written mid-read; oracle emits events
  before the document completes and errors at the later pull). The
  whole-tree reader parses at first Read() -> premature-EOF failure -> no
  events. Streaming/push reader work item (reuse probe/partial_delivery
  pause machinery; SP-14.3.1-6 style).
