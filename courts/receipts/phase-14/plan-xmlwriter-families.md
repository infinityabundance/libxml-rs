# plan-xmlwriter — engine root-cause FAMILY map (19 head tests)

Method: read each failing `.phpt` + its captured `*.diff`/`*.out` in `phpbuild-c`
(`/srcb/php-src/ext/xmlwriter/tests/`). Note: the task brief says "18", but the
candidate head list actually contains **19** (006…012 = 6, OO_006…OO_011 = 6,
bug41287/bug41326/bug71536/bug79029 = 4, toMemory_flush, toStream_shiftjis,
write_attribute_ns_basic_001 = 3) — matches SP-14.3.6 "19". OO_* tests are pure
OO wrappers that call the same libxml `xmlTextWriter*` builtin, so every OO_*
diff is byte-identical to its procedural twin and they are folded into the same
root-cause family (each OO duplicate is counted as its own member below).

Observed divergence (candidate line first in mental read is the `+` under the
`-` oracle line in each `.diff`).

---

## W1 prefixed closing-tag prefix lost after StartElementNS / WriteElementNS — count 8 | output
Members (procedure / OO twin):
- `006.phpt` `/OO_006.phpt`: `</bar>` vs `</foo:bar>` (inside `write_element_ns`)
- `007.phpt` `/OO_007.phpt`: `</child1>` vs `</ns1:child1>`
- `011.phpt` `/OO_010.phpt`: `</child1>` vs `</ns1:child1>` (write_attribute_ns)
- `012.phpt` `/OO_011.phpt`: `</child1>` vs `</ns1:child1>`
Open tag `xmlns:ns1=...` and attributes are byte-correct; only the END tag drops
the prefix.
**Inferred engine root cause:** `XmlTextWriter.elem_stack` stores `(prefix,name)`
pairs, but the end-element / endElement path rebuilds the close tag from the
local `name` only (or from a prefix that was emptied once the element leaves the
`StartElementNS` entry point), so an NS-declared element closes bare
(`</child1>`). Upstream `xmlTextWriterEndElement` emits `prefix:local`.
Surface: `src/xml/writer/mod.rs` end/close. **Output parity**, no crash.

## W2 empty-element / self-close serialization + sibling indentation nesting + plain-element attribute-namespace decl loss — count 4 | output
Members:
- `bug41287.phpt`: children placed INSIDE the prior `<foo></foo>`/`<bar xmlns=...>`
  instead of as following siblings; empty `<foo></foo>` never emitted then closes
  `<test>` as `<test>`(prefix lost too).
- `bug41326.phpt`: `<foo2></foo2>` is never closed before `<foo3/>`; `<foo3/>` and
  `<bar/>` are emitted INSIDE `<foo2>`; `<foo/>`(element with no content &
  full_end) left open briefly.
- `xmlwriter_toMemory_flush_combinations.phpt`: un-closed `startElement('foo')`
  flush returns `"<foo>"` (5) instead of oracle `"<foo"` (4, tag left OPEN), and
  `endElement` flush returns `"<foo></foo>"` (11) instead of `"<foo/>"` (6): the
  empty element is mangled/completed too early and not self-closed.
- `xmlwriter_write_attribute_ns_basic_001.phpt`: `write_attribute_ns` on a plain
  `start_element('root')` emits `prefix:id="elem1"` but DROPS the required
  `xmlns:prefix="http://www.php.net/uri"` decl (decl emission only wired for
  NS-typed element open, not for attribute-ns on a non-ns element), plus a stray
  ` </root>` leading-space on close.
**Inferred engine root cause:** the writer's node-delta / element-latch bookkeeping
(whether the incoming tag was left open as `<name`, the `is_empty`/empty-leaf
state, and where a following element/indent is appended) is wrong: indentation is
written as literal child content instead of layout, an empty `<x/>` is completed
too early or not at all, siblings nest under a preceding empty element, and the
default/attr namespace decl is only queued when the *element* was opened with NS.
Surface: `src/xml/writer/mod.rs` `write_indent`, empty-close, `pending_ns` gating.
**Output parity** (but sibling-nesting form can desync the node stack → watch for
crash in large docs). Recommended with W1 (shared element-close/latch state).

## W3 DTD child declarations (StartDTDElement/Attlist/Entity) drop their wrappers under indent — count 2 | output
Members: `008.phpt`, `OO_008.phpt` (identical diff). Candidate emits only the raw
body text concatenated (`valelem2*attr1  CDATA  #required...`) and loses every
`<!DOCTYPE ../ <!ENTITY | <!ELEMENT | <!ATTLIST ...>` open & close marker written
by the start/end DTD-child builtins.
**Inferred engine root cause:** `xmlTextWriterStartDTDElement/StartDTDAttlist/
StartDTDEntity` + their `End*` write the declaration wrapper with the element name
latched from the *body* `xmlwriter_text` instead of the Start* argument, so nothing
is emitted until a `WriteString`; with indentation the `[`/`]` bracket + name are
dropped and only text flows. Partial overlap with already-fixed residuals
R-000152 / R-000156 but those closed the no-indent composition; the indented
start/end-with-body path still loses markers. Surface: writer DTD state machine.
**Output parity**; DTD-only, no overlap with the serialize-NO_DECL save (DTD is
built-in writer, not xmlsave) — isolated.

## W4 PI / CDATA / text body of a leaf element moved onto its own indented lines — count 2 | output
Members: `009.phpt`, `OO_009.phpt` (identical diff). Expected `<pi><?php … ?>%w</pi>`
and `<cdata><![CDATA[<>&"]]></cdata>` all inline on the open-tag line; candidate
emits `<pi>\n<?php … ?>\n</pi>` (body at column 0) and `<cdata>\n<![CDATA[…]]>`.
Comments (`<!--…-->`) stay inline correctly.
**Inferred engine root cause:** `write_indent` runs before the PI/CDATA body
content and before the leaf's end tag, treating the immediately-following text as a
new element to indent, instead of writing the OpenTag+content+CloseTag of a leaf
atomically. Upstream `xmlTextWriterStartPI`/`StartCDATA` do not auto-indent and
`WriteString` appends on the same physical line. Surfaces independently of W2
(no comment affected) but same `write_indent` entry point. **Output parity.**

## W5 toStream output-encoding (declared SHIFT_JIS) is not transcoded — count 1 | output
Member: `xmlwriter_toStream_encoding_shiftjis.phpt`. Expected after
`startDocument(encoding:"SHIFT_JIS")` an SJIS-encoded/comment-normalized document
(`<!---->`); candidate writes the raw UTF-8 kana comment (`<!--ぁぁぁ-->`).
**Inferred engine root cause:** writing content through the PHP output-stream path
after a declared non-UTF8 encoding does not install/route through the writer's
byte-encoder (`xmlOutputBufferWrite` conv path that R-000151 describes), so bytes
are emitted in UTF-8 regardless of the declared doc encoding. **FLAG overlap:
this is the windows-1252/shift_jis/euc-jp input-encoding/transcode surface
(`src/xml/parser/input.rs` + xmlIO encoder).** Output parity. Independent of
W1–W4. Recommend touching alongside the L2/encoding family (shared encoder).

## W6 writer object/stream lifecycle at the php_libxml boundary (bulk of "UAF"/use-polish) — count 2 | crash/binding (message+return)
Members:
- `bug71536.phpt`: `die('now')` inside a half-open (openUri php://memory +
  mid `startElement`) XMLWriter destructor raises
  `Error: Invalid or uninitialized XMLWriter object` instead of printing `now`.
  Downstream of php_xmlwriter freeing a writer still carrying an un-flushed
  in-memory buffer doc / open uri on object destruction.
- `bug79029.phpt` (also needs xmlreader): oracle lets `fclose` on the writer/
  reader-owned stream fall through to the friendly
  `cannot close the provided stream, as it must not be manually closed` warning;
  candidate has already hard-closed the underlying php stream so `fclose`
  throws `TypeError: must be an open stream resource`. **FLAG overlap with the
  ext/xmlreader UAF (#79029 family).**
**Inferred engine root cause:** buffer/stream ownership marking + write-destroy
ordering at the php binding boundary (`php_libxml` stream registrar vs
`xmlOutputBufferClose`/writer free). Binding-level error-return parity —
distinct from WT 1-5. Crash-class (UAF target per title) → treat as ownership.

---

## Prerequisite order (xmlwriter-internal)
1. W6 lifecycle/ownership first — any teardown crash/binding error aborts a run
   before serialize parity is even observable (bug79029 is an explicit UAF title;
   bug71536 errors out mid-run).
2. W1 + W2 together: they share the element-close/latch/prefix/empty state in
   `elem_stack`/`write_indent`. W1's end-prefix restore should land in the same
   commit as W2's empty-leaf sibling handling (many tests carry BOTH signatures,
   e.g. 007/OO_011 prefix loss AND `<empty>      </empty>`; deferring one breaks
   the other's green-up).
3. W3 DTD markers (isolated DTD state machine; safe independently).
4. W4 leaf PI/CDATA indent (independent `write_indent` branch).
5. W5 encoding last (independent, but shared encoder with the 
   windows-1252/shift_jis/euc-jp input-encoding family — coordinate so the
   encoder change doesn't regress `toStream_encoding_utf8`/`normal_usage`).

## Overlap flags (cross-extension)
- W2 namespacing/empty tag & the `prefix`-close = also touched by getElementsBy…
  no: the ONLY shared surface is `xmlsave` NO_DECL empty-form serializer only if a
  save doc walks writer output — here writer writes directly; no xmlsave NO_DECL.
- W5 ↔ input-encoding (windows-1252/shift_jis/euc-jp) + xmlIO encoder (shared).
- W6 ↔ ext/xmlreader #79029 UAF + dom stream/teardown (adopt/destroy).

Verification note: bodies of 006…OO_011/008/bugs/flush/shiftjis/attrns_basic and
their `.diff` were inspected in the container (all 19). No member left unverified.
