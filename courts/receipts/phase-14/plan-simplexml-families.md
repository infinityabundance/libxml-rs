# plan-simplexml — engine root-cause FAMILY map (9 head tests)

Method: read each failing `.phpt` body + captured `*.diff`/`*.out` in `phpbuild-c`
(`/srcb/php-src/ext/simplexml/tests/`). All 9 bodies + diffs inspected (no member
unverified). OO-SimpleXML output is `var_dump` of the produced tree / warnings, so
each divergence is observed directly.

---

## S1 LIBXML_RECOVER diagnostic presentation suppressed — count 1 | warning-only
Member: `xml_parsing_LIBXML_RECOVER.phpt`. Parsing `<root><child/>` (missing
`</root>`) with `LIBXML_RECOVER` returns the correct recovered child object, but
the candidate emits NONE of the 3 oracle messages
(`simplexml_load_string(): %s`, the raw `<root><child/>` context line, and the
`^` caret line).
**Inferred engine root cause:** with `XML_PARSE_RECOVER`, the premature-EOF /
"not well-formed" recovery path on the candidate presents no
`xmlCtxtReadMemory`-routed generic error + source-context + caret markup, which
PHP surfaces verbatim. Surface: recovery parse-error raising in the parser/state
(probably does not set the "recovered, emit structured error" message with the
snapshot). **Warning/message parity only** — tree shape already green.
Prereq: none. **FLAG overlap**: identical `LIBXML_RECOVER` warn-text expected by
ext/xml, ext/xmlreader and ext/dom (`src/xml/parser/options.rs` net; see dom L1
RECOVER warn). One engine fix must not regress those.

## S2 LIBXML_NO_XXE does not stop NOENT+SYSTEM general-entity load — count 1 | warning-only (+option semantics)
Member: `xml_parsing_LIBXML_NO_XXE.phpt`. Doc with internal `<!ENTITY foo` +
`<!ENTITY xxe SYSTEM "file:///etc/passwd">` then `<set>&foo;&xxe;</set>` under
`LIBXML_NOENT | LIBXML_NO_XXE`. Object result matches (has `foo => "bar"`), but
candidate still attempts the `&xxe;` external fetch and emits an extra
`I/O warning : failed to load "file:///etc/passwd"` — oracle suppresses because
NO_XXE blocks substitution of the external general entity while still expanding
`&foo;`.
**Inferred engine root cause:** NO_XXE gating only on DTD/parameter/entity-*decl*
fetches, not on substituting an already-declared external (SYSTEM) general entity
under NOENT (upstream `xmlParseReference`/`entity->URI` check consults NO_XXE and
does not call the entity loader). Surface: parser entity-reference substitution +
options. **Warning parity / option-semantics** (implies possible side-channel if
the file DID exist — XXE relevant). **FLAG overlap** with ext/xml + ext/xmlreader
+ dom L1 ("NO_XXE external-entity still attempted") — shared options.rs/entity.

## S3 XPath invalid-expression error message prefixed "XPath error : " (double) — count 1 | message-only  [CLOSED 2026-09-03]
Member: `008.phpt`. `simplexml$xpath("**")` → oracle `XPath expression must return
a node set…`/`Invalid expression`; candidate emits
`XPath error : Invalid expression` (raw engine code string re-prefixed).
The `***` number-returned branch already matches. Also captures the API not-you-put
prefix rust.
**Inferred engine root cause:** php's `xpath`-error helper prints upstream's
`last_error` line which on the candidate already carries an `XPath error : `
prefix (and a full path), so the binding's own "Invalid expression" add-on
becomes doubled/misworded. Message-text parity at the XPath error-string
boundary. Kind **E/message-only**; independent of tree. 
**Closed:** xpath.c xmlXPathErrFmt channel selection — with no structured
handler the message goes VERBATIM to the generic channel (xmlGenericError is
NOT one of the parser channels that trigger xmlFormatError's fragment
stream), so raise_xpath_error now delivers
`GenericDelivery::Custom(generic func, ctx)` when php installed one (Stream
fallback preserves the console fragment stream). Receipt:
php-14-3-simplexml-xpath-error-20260903/; guard:
test_xpath_compile_error_verbatim_to_generic_channel; probe:
consumers/xpeval-probe.c.

## S4 PI content trailing space trimmed (string() of processing-instruction) — count 1 | string-only
Member: `gh12167.phpt`. `<?foo pi contents ?>` → expected `pi contents ` (keeps
the single trailing space before `?>`); candidate `pi contents` (length 11).
**Inferred engine root cause:** PI content extracted by XPath
`processing-instruction()` strips the trailing whitespace that precedes `?>`;
upstream retains text up to the closing `?`. Surface: parser/serializer PI content
boundary (tokenizer emits PI body excluding one trailing byte the oracle keeps) or
the XPath libxmlGetThreadId-… node-string() of the PI. Overlaps xmlreader/PI families.
Kind string/message-only. Independent.

## S5 empty-string child keeps an empty text node → `<bar></bar>` not `<bar/>` — count 1 | output
Member: `bug76712.phpt`. `addChild('bar','')` should add NO text child → serialize
`<bar/>`; candidate appends a 0-length text node → `<bar></bar>`. The later
`addChild('bar')` + `$sxe->bar=''` case already `<bar/>`-correct (matches).
**Inferred engine root cause:** `xmlNodeAddContent/xmlNodeSetContent` with an
empty value creates/retains an empty text child that the serializer then forces to
`<x></x>`; oracle drops the empty text node (or AddChild("",…) adds none). Empty-
element vs empty-text-child serialize decision. **Output.** **FLAG overlap:**
same serializer empty-form as dom S1/M1 & xmlwriter W2 empty `<x/>`; fix in save/
empty-node discipline must not regress those.

## S6 addChild entity-decoding asymmetry for new-vs-existing content — count 1 | output/escaping
Member: `bug44478.phpt`. `addChild('node2','a &#38; b')` → oracle later reads
`a & b` and saves `<node2>a &amp; b</node2>` (the `&#38;` in the create-content
path is decoded); candidate keeps the literal `a &#38; b` (reads it raw, saves
`<node2>a &amp;#38; b</node2>`). The property-assignment path (`= 'a &#38; b'`)
already matches oracle.
**Inferred engine root cause:** divergence between the engine call the binding uses
to create a NEW node's content (`xmlNewTextChild`/add-Child with a fresh text node
→ content taken as parsed source / XML-parsed) vs assignment on an EXISTING node
(`xmlNodeSetContent` → content taken as raw). Engine treats the two new-content
paths differently (`&#38;` decoded on one, raw on the other); oracle surfaces the
same text both ways. Surface likely `xmlNewTextChild`/tree add-child vs set-content
escaping. Output/escaping. **FLAG overlap** with dom entity/escaping rows
(`xmlBufAttrSerializeTxtContent` / entities) and xmlwriter attr escaping. Rather
leaf; independent.

## S7 clone of SimpleXMLElement not detached → mutation lands on the original doc — count 1 | output (ownership-adjacent)  [CLOSED 2026-09-03]
Member: `bug63575.phpt`. `clone $o1`; `current($o1-clone->xpath('/a'))->addChild('c',…)`
must leave `$o1` = `<a><b/></a>` and mutate only `$o2` → `<a><b/><c/></a>`. Candidate
mutates the ORIGINAL `$o1` (prints `<a><b/><c></c></a>`) and leaves `$o2` without
`<c>`, plus `<c></c>` full-form instead of `<c/>`.
**Inferred engine root cause:** SimpleXML `__clone` deep-copies via
`xmlDocCopyNode`/`xmlCopyProp` but the resulting node's ownerDoc/root-parent chain
is not detached, so the XPath `/a` returned node still points into the original
document (candidate `xmlCopyDoc` keeps ancestor as the source parent — the inverse
of dom Fix-3/R-14.3-COPYDOC-ROOT-PARENT). addChild then appends to the source doc;
empty `<c/>` form also picks the S5 serializer issue.
**FLAG overlap** heavily with dom import/clone (`DOMDocument_importNode`, R-14.3
copy/append) — fix the copy-owner/root-detach engine once, re-run both.
Kind output; potential double-free risk if both docs free the shared child →
ownership (validate HOSTILE-ABI/FREEPROP like Fix 3/4).
**Closed:** copies no longer carry `_private` (upstream xmlStaticCopyNode /
xmlCopyNamespaceList; php keys wrapper registrations on `_private`). Root-
element clones (xmlCopyDoc) now bind to their own document: XPath and
mutations resolve into the clone (bug63575 PASS). Receipt:
php-14-3-copy-private-20260903/; guard:
test_copies_do_not_carry_private; probe:
consumers/clone-xpath-probe.php (candidate == oracle).

## S8 percent-encoded NUL in URI: loader warnings/return path degraded — count 1 | output+warning (test-secondary crash)  [CLOSED 2026-09-03]
Member: `bug79971_1.phpt`. `simplexml_load_file("file://…/%00foo")` must emit the
two `URI must not contain percent-encoded NUL bytes` / `I/O warning : failed to
load %s` warnings and return false; candidate suppresses the NUL/IO warnings and
later the test hits `$sxe->asXML()` on `false` (Fatal in the phpt, secondary
effect of missing warning routing).
**Inferred engine root cause:** the percent-encoded-NUL URI rejection and its
warning live in the ext/xmlIO loader path (php percent-NUL guard + xmlIO failure
warning routing). On candidate the loader surface that normally detects `%00`
and emits the two warnings is bypassed (returned false through a branch that
suppresses diagnostics). Surface: URI filename loader error routing
(`xmlReadMemory`/`xmlParserInputBufferCreateFilename`/file-open error text). 
**FLAG overlap** with dom L2 NUL-filename / empty-file and shared xmlIO loader rows.
**Closed:** engine filename-input creation now consults the registered
`xmlParserInputBufferCreateFilenameDefault` (php streams loader) at all 11
file-open sites (upstream xmlNewInputFromUrl semantics); `file://` URIs
load, the php percent-NUL guard + "I/O warning : failed to load" report
fire, and asXML's %00 output side was already routed by EXT-6. Receipt:
php-14-3-input-loader-routing-20260903/; guard:
test_main_doc_open_consults_registered_input_loader; probes:
consumers/nul-uri-probe.php + missing-file-probe.php (candidate == oracle).

## S9 XSLT transformToDoc→SimpleXML result empty on autovivification — count 1 | output (xsl/dom interplay)
Member: `gh17153.phpt`. `transformToDoc($sxe, SimpleXMLElement::class)` then
`$result->h = "x"` → oracle object retains real h1/h2/hr children + h; candidate
object only has `h`, i.e. the transform result DOM reached SimpleXML wrappers
childless / autovivification attached to an empty root.
**Inferred engine root cause:** XSLT result-document → SimpleXML wrapping loses
the transformed top-level children (result tree lives in an intermediate doc not
adopted as the root the PHP wrapper iterates, or `transformToDoc` returns a
shell with no children on candidate). **FLAG overlap** with ext/xsl SP-14.3.7
transformTo*/result lifetimes and dom result classes — genuinely an xsl-engine /
result-ownership issue surfaced here. Recommend sequencing with xsl family, not a
simplexml-internal root.

---

## Prerequisite order (simplexml-internal + cross)
1. S7 clone detachment (ownership-adjacent; shares the copy/root-parent engine fix
   with dom Fix-3 clone/import — do once, keep both green).
2. S2 NO_XXE + S1 RECOVER parse-option diagnostics together (one options.rs/patch;
   must not regress ext/xml, xmlreader, dom L1 → run all).
3. S8 NUL-filename/loader error routing (xmlIO shared; own fix).
4. S5 empty-text-child + S6 addChild-escape (serializer/set-content discipline;
   S5 overlaps empty-form used by S7 too).
5. S3 XPath error text, S4 PI trailing space — pure message/string polish (cheap).
   → both CLOSED 2026-09-03 (receipts php-14-3-simplexml-xpath-error-20260903/
   + php-14-3-simplexml-pi-data-20260903/).
6. S9 xsl result — move to SP-14.3.7 xsl sequencing (not sim-first).

## Overlap ledger
NO_XXE/RECOVER (>dom/xml/xmlreader), empty-`<x/>` serializer (dom S1/M1 + xmlwriter
W2), clone/import-owner (dom copy/clone), loader NUL + xmlIO (dom L1/L2), PI string
(xmlreader/`?` PI), xsl result -> xsl. No xmlwriter-specific overlap beyond the
shared serializer empty-form/escaping surfaces. SimpleXML-only surfaces: none of
the 9 are message-free crashes under engine (all verified against diffs); S7 is the
only ownership-risk candidate.
