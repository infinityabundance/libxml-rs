# Phase 14.8 — xmlreader cursor/attribute + DTD-validate paths (86 → 77)

Date: 2026-09-04 · Commit: `bd62a97` · Gate log: `phpbuild-c:/out/xpe-six23.log`
Diff vs xpe-six22: `OLD=86 NEW=77 FIXED=9 NEW_ONLY=0`

## Flipped
xmlreader: `003-get-errors`, `003-move-errors`, `008`, `010`, `012`,
`015-get-errors`, `bug42139`, `next_basic` + dom
`modern/extensions/attribute_renaming_conflict` (bonus, from the ATTLIST
multi-attr parse fix).

## Root causes / fixes (all oracle-probed, php state machine verified first)
1. `xmlTextReaderNext`: upstream xmlreader.c xmlTextReaderNext semantics —
   a NON-element current node (text/END/…) degrades to a plain Read (one
   step in document order, may land on the parent's END_ELEMENT, returning
   TRUE — 010); an element START skips its subtree (children + its own END
   + ancestor ENDs) to the next sibling; when nothing remains the cursor
   CLEARS to EOF (name `''`, type NONE, state END) and returns 0 —
   next_basic `next('node5')`. Old code parked the cursor on the current
   node for the 0 case and never positioned END events from non-elements.
2. `MoveToNextAttribute`: from an ELEMENT it must move to the FIRST
   attribute (upstream "if the current node is an element, this moves to
   its first attribute") — 003-* iterate attributes without a prior
   MoveToFirstAttribute. Old code returned -1 (element cursor) → false.
3. `MoveToAttributeNo`: upstream resets `reader->curnode = NULL` (element)
   on ENTRY, so a failed by-number lookup leaves the reader on the ELEMENT
   — php comment "node pointer moves back to the element in this case".
4. Reader `cache_name_and_value`: XML_DTD_NODE (14) never matched the
   named-type list → DOC_TYPE nodes printed name `''` (bug42139). The
   doctype name (`root`) is now reported.
5. `xmlTextReaderIsValid`: the reader frees its parser context after the
   parse, so validity was lost (stub returned 0 forever). Now snapshots
   `ctxt->valid` + `ctxt->validate` before the free and reports per
   upstream: 1 when a validating parse finished clean, 0 when validation
   wasn't performed or failed → 008's `file DTD: ok` / `string DTD: ok`.
6. `resolve_dtd_path` now resolves `file://` system ids to
   percent-decoded local paths (php rawurlencode's absolute DTD paths,
   `file:///%2Fsrcb%2Fphp-src%2F...`) — the reader string-DTD pass of 008
   loads dtdexample.dtd (its `file:///...` id previously resolved to None →
   every element "No declaration").
7. `parse_attr_default`: the quoted-default branch consumed `rest.len()`
   (and #FIXED consumed even more) — after the FIRST quoted default in a
   multi-attribute `<!ATTLIST>` every later attribute was silently dropped
   (012.dtd `bar CDATA '' baz CDATA ''` lost `baz`). Consumed-lengths now
   cover exactly the quoted literal. This is also what fixed dom
   attribute_renaming_conflict.

## Validation
- Oracle-vs-candidate reader probe (read/next/attr sequences) is
  byte-identical.
- cargo test --lib 1241 pass / 1 ignored (one reader unit test updated to
  the verified oracle Next semantics); fmt clean.
- Full six-extension gate xpe-six23.log: 77 failed (was 86), zero new.

## Remaining xmlreader (8): 007/013 (setRelaxNGSchema/setSchema attach +
  error text), bug64230 (internal error text mangling), bug73053 (schema
  errors), gh19098 (xmlns attrs), fromStream_custom_constructor /
  fromString_custom_constructor (property values with custom subclass),
  fromStream_broken_stream. Then dom (51), xsl (16), simplexml 1,
  xmlwriter 1.
