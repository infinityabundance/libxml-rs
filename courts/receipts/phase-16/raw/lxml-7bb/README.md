# lxml court 7bb — HTML name case, raw-text head placement, frameset, boolean attributes

## Root causes fixed

### 1. HTML tag/attribute names were not lower-cased

Upstream `htmlParseHTMLName` (HTMLparser.c) lower-cases every element and
attribute name on read — "HTML names are not case-sensitive". Our parser kept
the source spelling, so `<HTML><BODY><P>` produced `HTML`/`BODY`/`P` tags and
`test_tags_upper_lower_case_html` (and any case-insensitive consumer) saw false
differences.

`new_element_node` and `attach_attrs` now ASCII-lower-case the name, covering
the whole-document tree builder, the raw-text path and the SAX/push seam.

Fixes `test_tags_upper_lower_case_html`. The internal
`xml::html::tests::test_mismatched_case` unit test asserted the old
(case-preserving) behaviour and was corrected.

### 2. A leading `<style>`/`<script>` was placed in `<body>`

The raw-text branch of the tokenizer decided its insertion point from
`ctxt.in_head` alone, which is false before any head content, so it always
called `ensure_body`. Upstream runs the head/body inference before creating
the element, so a leading raw-text head element lands in the implicit
`<head>`.

The raw-text branch now consults the tag's `HTML_HEAD` flag and
`seen_body_content`, like `handle_start_tag`.

Fixes `doc/test_rewritelinks.txt`.

### 3. `<frame>`/`<frameset>` implied `<body>`

Upstream `htmlCheckImplied` excludes `frame`, `frameset` and `noframes` from
body creation; a frameset document puts them directly under `<html>`. Our
parser treated them as ordinary body content and wrapped them in `<body>`.

A `frame_container` case now materialises `<html>` (when nothing else is open)
without `<body>`, keeping an enclosing `<frameset>` as the parent for nested
`<frame>`.

Fixes `test_parse_fromstring` (lxml.html frames).

### 4. HTML output did not minimise boolean attributes

`htmlAttrDumpOutput` (HTMLtree.c) writes a boolean attribute in minimised form
(`selected`, never `selected="..."`) — XSLT 1.0 16.2. Our HTML serializer
always wrote `name="value"`, so `xf.write(elt)` in HTML mode emitted
`selected="bar"`.

The HTML serializer now consults the existing `htmlIsBooleanAttr` port.

Fixes `doc/test_forms.txt` and `test_xml_mode_write_inside_html`.

## Evidence

* Full suite: **12 raw unique failing ids**, down from 17 — **5 fixed,
  0 regressions**:
  * `doc/test_forms.txt`
  * `doc/test_rewritelinks.txt`
  * `test_parse_fromstring`
  * `test_tags_upper_lower_case_html`
  * `test_xml_mode_write_inside_html`
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: 1250 passed / 0 failed.
* `failing-ids.txt` is the raw unique id set; the filtered delta against the
  pre-slice baseline is exactly the five ids above.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
