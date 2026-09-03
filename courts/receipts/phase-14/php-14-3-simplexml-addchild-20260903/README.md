# simplexml S5/S6 — xmlNewChild content parsed as attribute value

Closed 2026-09-03. Full suite **241 → 239**, zero regressions. simplexml 7 → 5
(bug44478 + bug76712 PASS); dom 152 / xml 0 / xmlreader 29 / xmlwriter 1 (W5) /
xsl 52 unchanged. Log: phpbuild-c:/out/xnc-six.log (1291-run, 239 failed /
40 skipped).

## Root cause (mirrors tree.c xmlNewChild → xmlNewDocNode → xmlNewElem)

The exported `xmlNewChild` created the element with `new_text(content)` — the
content stored as RAW text and an EMPTY content appending a 0-length text
child. Upstream parses the content as an ATTRIBUTE VALUE
(`xmlNodeParseAttValue`, attr = the new element):

- `value[0] == 0` → early out, NO children → addChild('bar','') serializes
  `<bar/>` (bug76712);
- character references decode → `addChild('node2','a &#38; b')` stores
  `a & b`, reads back `a & b`, saves `a &amp; b` (bug44478);
- declared general entities become ENTITY-REF children (saved as `&name;`);
- a bare `&` with no terminating `;` consumes the `&` and keeps the rest as
  text (`x & y` → `x  y`).

The crate's own `xmlNewDocNode` already routed content through the faithful
`node_parse_att_value`; the exported `xmlNewChild` (simplexml's addChild
target) was a divergent raw-text twin.

## Fix

- `xmlNewChild` now mirrors upstream: type-check the parent, inherit the
  parent ns on elements, build the element via `xmlNewDocNode` (the
  attr-value parse) and link it directly at the parent's tail.
- `exports_tree.rs node_parse_att_value` hard-failed on a bare `&` without a
  `;` (`failed = true` → -1), diverging from its `exports_string.rs` twin and
  upstream (tree.c `break` → remainder flushed as text). Now breaks and
  continues-as-text, so `xmlNewDocNode`/`xmlNewChild` accept `x & y` like the
  oracle.

## Guard

exports_xml2 `test_xml_new_child_parses_content_as_att_value`: `""` → no
child; `a &#38; b` → single text child `a & b`; `x & y` → text `x  y`.

Probe: consumers/addchild-probe.php — php pin, oracle-equal on all four
content shapes (empty / bare `&` / `&#38;` / declared `&e;`).
cargo test --lib 1230 pass / 1 ignored; clippy no new warnings; fmt clean.
