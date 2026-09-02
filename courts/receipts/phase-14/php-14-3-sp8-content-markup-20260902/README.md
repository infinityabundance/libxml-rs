# SP-14.3.1-8 + KEY-2 — content-`<!`-markup rule + push default-markup closure

Closed 2026-09-02. Full suite **283 -> 276** (dom 169 -> 166, xml 5 -> 1;
zero regressions elsewhere). This is the sealed SP-14.3.1-8 atom: the earlier
tokenizer-only attempt regressed two dom fragment tests; KEY-2's content-`<!`
rule is the engine fix that makes the push/EOF edits non-regressing.

## SP-14.3.1-8 root cause (gh20439_1/2, bug27908, bug46699)

PHP expat-compat emits default-handler raw markup by seeking back from
`xmlParserInput->cur` to the tag's opening `<`. Two candidate defects made that
seek dereference a dangling pointer / emit nothing on a per-byte `xml_parse`
feed:

1. The push context (`xmlCreatePushParserCtxt`) wired the C `ctxt->input` to the
   empty constructor buffer; the accumulated buffer is rebuilt on every
   `xmlParseChunk`, so `base` went stale. `sync_input_position()` now repoints
   `_xmlParserInput` base/cur/end/line/col from the tokenizer's LIVE buffer at
   every event (`consumed` zeroed so the compat byte index stays `cur - base`;
   bug26614 stays at 96).
2. EOF-in-construct was delivered as text: a chunk ending right after `<`/`<!`
   or inside a comment previously returned `Characters("<")`/`Characters("<!")`.
   A trailing `<` now routes through `scan_start_tag` (records NAME_REQUIRED,
   marks unterminated); `Comment` tokens carry `unterminated`; unterminated
   constructs pause (`truncated_abort`) in silent probes / eager-partial
   delivery instead of firing partial events.

## KEY-2 root cause (content-`<!`-markup rule)

A `<!` that is not `<!--`, `<![CDATA[`, or a prolog `<!DOCTYPE>` is an INVALID
element start in element content: upstream `xmlParseStartTag` fails the name at
the `!` with `XML_ERR_NAME_REQUIRED` (68) and clears `wellFormed`. The
candidate tokenizer's `scan_markup_decl` swallowed `<!ENTITY …>/<!ELEMENT …`
as `Characters` text (wf stayed 1), and a `<!DOCTYPE` in content raised
internal-error-1 with wf still 1. Both meant PHP's XML innerHTML/outerHTML
fragment writer accepted `'<!ENTITY foo "content">'` bodies (no "XML fragment
is not well-formed" exception, doc mutated) — the exact regression the raw SP-8
push edits produced on `Element_{inner,outer}HTML_writing_errors`.

## Fixes

- tokenizer `scan_tag_or_markup`: pre-screen `<`+`!` for comment / CDATA /
  DOCTYPE; everything else routes to `scan_start_tag` (empty name → 68 at the
  '!', oracle position).
- tokenizer `DocType` token now carries `unterminated` (`scan_doctype_body`
  tracks whether the depth-0 `>` arrived).
- state.rs `parse_prolog` DocType arm: truncated DOCTYPE pauses in probes.
- state.rs `parse_element` content loop: a complete DocType in CONTENT raises
  NAME_REQUIRED 68 FATAL (clears wellFormed via `raise_parser_error`);
  truncated one pauses in probes. `<!ENTITY`/`<!ELEMENT`/unknown `<!junk>` are
  handled by the tokenizer pre-screen (scan_start_tag → 68), so content never
  sees them as text.
- Internal-subset decls are unaffected: `scan_doctype_body` consumes the whole
  `[ ... ]` subset into the one DocType token; `parse_internal_subset` scans
  decl bytes with its own scanner, never the main tokenizer.

## Oracle-pinned probe (courts/suites/phase14/consumers/pushmarkup2-probe.c)

Push `<root>BODY</root>` with body in a separate chunk, then close; compare
wellFormed/errNo:

| body | oracle | candidate (was → now) |
|---|---|---|
| `<!ENTITY foo "content">` | wf=0 err 68 | wf=1 text → **wf=0 err 68** |
| `<!DOCTYPE html>` | wf=0 err 68 | wf=1 err 1 → **wf=0 err 68** |
| `<!ELEMENT x EMPTY>` | wf=0 err 68 | wf=1 text → **wf=0 err 68** |
| `<!-- ok -->` | wf=1 err 0 | wf=1 err 0 (unchanged) |
| `<![CDATA[ok]]>` | wf=1 err 0 | wf=1 err 0 (unchanged) |
| `hello <b>w</b>` | wf=1 err 0 | wf=1 err 0 (unchanged) |

## Measured (full six-extension, phpbuild-c, /out/k2-six-full.log)

- ext/xml 5 -> 1 (bug27908, bug46699, gh20439_1, gh20439_2 PASS;
  xml_error_string_basic_libxml remains = SP-14.3.1-9).
- ext/dom 169 -> 166: Element_innerHTML_prefixed_writing,
  Element_outerHTML_writing, innerHTML_cache_invalidation PASS; the
  *_writing_errors_* pair stays green (no regression); remaining F1 members
  (Element_innerHTML_writing, Element_innerOuterHTML_reading,
  Element_insertAdjacentHTML) are serializer-blocked ("Could not save
  document" — dom S1 family, pre-existing at HEAD).
- simplexml 9 / xmlreader 29 / xmlwriter 19 / xsl 52 unchanged.

## Guards

- tests.rs `test_push_incremental_eof_prefixes_not_text` (per-byte push: only
  real text delivered; fails at HEAD).
- tests.rs `test_content_markup_decl_clears_wellformed` (pull mode: ENTITY /
  DOCTYPE / ELEMENT / junk-in-content → null doc wf=0 err=68; comment/cdata/
  text and prolog-DOCTYPE-with-subset stay wf=1 err=0).
- cargo test --lib 1222 pass / 1 ignored; clippy no new warnings (4
  pre-existing); fmt clean.

## Receipts / evidence

- logs (phpbuild-c): /out/k2-six-full.log, /tmp/k2-xml.log, /tmp/k2-dom.log.
- probe: courts/suites/phase14/consumers/pushmarkup2-probe.c (kept).
- prior analysis that motivated this design:
  courts/receipts/phase-14/php-14-3-sp8-gh20439-analysis.md.
