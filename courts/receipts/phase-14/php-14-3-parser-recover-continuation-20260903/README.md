# php-14.3 parser recovery-continuation + xml-decl version + dump NULL-size (2026-09-03)

Full suite **207 → 201 failed**, ZERO regressions (name-level diff vs the
207 baseline `phpbuild-c:/out/xpe-six8.log`: 0 new). New log:
`phpbuild-c:/out/xpe-six9.log`. Oracle on all of these: 0 fails.

Flipped to PASS (all ext/dom; exact 6):

- `DOMDocument_load_error1_gte2_12` / `DOMDocument_loadXML_error1_gte2_12`
  (not_well_formed.xml — two mismatch reports in one pass)
- `DOMDocument_load_error2_gte2_12` / `DOMDocument_loadXML_error2_gte2_12`
  (not_well_formed2.xml — broken start tag `<book number=nine>` + stray
  `</book>` closes `<books>` with the 4th warning at **line 7**)
- `DOMDocument_load_error4` / `DOMDocument_loadXML_error4`
  (`<?xml version="3.1" ?>` — fatal XML_ERR_UNKNOWN_VERSION, load returns false)

## Engine root causes closed (src/xml/parser/state.rs, tokenizer.rs, tree)

### 1. FATAL-error scanning continuation (parser.c 2.15 xmlParseElementEnd /
xmlParseEndTag2 / xmlParseContentInternal)
- **Mismatched end tag**: a stray `</X>` inside element `Y` no longer stops
  the parse — upstream closes the CURRENT open element as if its own end tag
  appeared (the stray name only feeds the XML_ERR_TAG_NAME_MISMATCH (76)
  message: str1=open name, str2=close name, int1=open line), then scanning
  continues so LATER structural errors are reported too (error1's second
  mismatch). Closing the current element is recovery-INDEPENDENT: the fatal
  clears wellFormed (which decides doc retention), not scanning.
  The old code popped the wrong name stack in recovery mode.
- **Failed child start tag**: an unterminated StartTag token in element
  content never opens an element — upstream continues scanning the CURRENT
  element's content, so the stray end tag that closes it is still reported
  (error2's fourth warning). The old `pop_name(); return Err(())` aborted
  the whole content pass after the tokenizer's "Couldn't find end of Start
  Tag" (73) errors. Probes/partial deliveries still pause (`truncated_abort`).
- **Epilog skip**: when the root parse already failed (errNo set) it ends
  early — upstream xmlParseDocument leaves trailing bytes unparsed, so no
  diagnostics may fire from leftovers. `parse_epilog` now only runs when the
  tokenizer fully consumed the input (error2 must not gain a spurious
  5th "Extra content"-class warning from the bytes after the stray end tag).

### 2. PHP's `line: N` suffix reads `ctxt->input->line` at handler time
php_libxml_error_handler_va (php-src ext/libxml/libxml.c) prints
`parser->input->line`, NOT xmlError.line. The C-level xmlError.line was
already correct (= 7 for the code-76 raise on line 7); the mirrored
`parser->input` was stale (= 4, last synced by the preceding `characters`
event) because the broken start tag leaves the tokenizer scanning silently
across lines 4–7 with no SAX event in between. Upstream raises code 76 in
xmlParseEndTag2 AFTER `NEXT1` consumed the end tag's `>`, so
`ctxt->input` sits on the tag's own line when the handler fires.
Fix: `sync_input_position()` before the mismatch raise (same explicit-sync
pattern the validity raise in sax_end_element already used). Probe:
`consumers/input-line-at-raise-probe.c` now prints oracle-identical
`inputline=7` for code 76 (both engines). The FILE-load variant had never
pinned this (its EXPECTF `%s` swallows the suffix); loadXML's did.

### 3. XML-declaration version ladder (parser.c xmlParseVersionInfo /
xmlParseVersionNum / xmlParseXMLDecl)
The candidate stored any version silently. Upstream semantics, now recorded
by the tokenizer at scan position (record order == upstream raise order, so
errNo keeps ending on XML_ERR_XMLDECL_NOT_FINISHED 57 for unterminated
decls — KEY-3 guard `<?xml version="dummy">` stays green):
- VersionNum = `<digit> '.' <digit>*` (exactly one leading digit: "10.5" is
  NOT a version number). A scan stop INSIDE the literal = missing closing
  quote: XML_ERR_STRING_NOT_CLOSED (34) "String not closed expecting \" or '".
- version NULL → XML_ERR_VERSION_MISSING (96) "Malformed declaration
  expecting version".
- parsed prefix != "1.0": XML_PARSE_OLD10 → fatal; "1.x" → warning
  (XML_WAR_UNKNOWN_VERSION 97); otherwise (e.g. "3.1") fatal
  (XML_ERR_UNKNOWN_VERSION 108) — wellFormed cleared, doc not kept, the load
  fails (error4). The message shows the PARSED prefix ("1.x" reports '1.').
- The tokenizer gained an `old10` flag (parser sets it from
  `ctxt->options & XML_PARSE_OLD10`, like `set_max_name_length`).
C probe: the 11-case version-literal ladder is code/level/message/doc
identical to the oracle (probe in the receipt log).

### 4. xmlDocDumpFormatMemory / xmlDocDumpMemory tolerate a NULL length pointer
Upstream writes `*mem` regardless; only `*size` is conditional
(xmlDocDumpFormatMemoryEnc). The candidate returned early when `size == NULL`
leaving `*mem` unwritten — the recover probe's
`xmlDocDumpFormatMemory(d, &s, NULL, 0)` never produced the dump.

## Probes kept (consumers/, candidate == oracle byte-for-byte)
- `recover-cont-probe.c` — not_well_formed.xml structured capture ± RECOVER
  (RECOVER now materializes the same recovered tree + dump as the oracle)
- `broken-start-tag-probe.c` — not_well_formed2 SAX events/errors
- `input-line-at-raise-probe.c` — xmlError.line vs ctxt->input->line at each
  raise (php suffix parity contract)

## Tracked residuals (out of this closure's scope)
- Candidate `ctxt->input->col` at raises with no preceding SAX event is stale
  (probe `i`: inputcol=2 vs oracle 15/15/15/9; code 76's col matches). php
  prints only `line`, so no phpt pins col; structured-error col parity is a
  future atom.
- `<?xml version="dummy"?><r/>` (invalid version literal + later content):
  upstream keeps scanning from the failure char INSIDE the literal and adds
  65/57 trailing diagnostics; the candidate (whole-value tokenizer scan) stops
  at [34, 96]. No test pins it.
- html parse-side `&eacute;` entity table gap (html/mod.rs HTML_ENTITIES) and
  gh19612's declared-entity attr `&foo;` retention remain separate families.
- xmlreader `003-get-errors` / `015-get-errors` remain red (pre-existing;
  not touched by this closure).

Validation: cargo test --lib 1239 pass (1 ignored), clippy back to the 4
pre-existing warnings, fmt clean. Full six-gate `phpbuild-c:/out/xpe-six9.log`:
1291 tests / **201 failed** / 40 skipped (skips == oracle).
