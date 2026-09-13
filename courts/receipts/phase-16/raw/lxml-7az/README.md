# lxml court 7az — attribute-name dictionary interning and XInclude xml:base fixup

## Root causes fixed

### 1. `xmlSetNsProp` did not intern the attribute name in the document dict

lxml's `_MultiTagMatcher` resolves a `{uri}name` matcher through
`xmlDictExists(doc.dict, name, len)` and then compares the node/attribute name
by POINTER (`_tagMatchesExactly` compares `c_node.name is c_qname.c_name`).
Attributes created through `xmlSetNsProp` stored a plain `dup_xml_str(name)`,
so the name was never in the dict: `_mapTagsToQnameMatchArray` skipped the tag,
`rejectsAllAttributes()` reported nothing to strip, and
`etree.strip_attributes` / `objectify.deannotate` silently became no-ops.

Upstream `xmlNewPropInternal` interns the name with `xmlDictLookup` when the
node's document has a dictionary. `set_ns_prop` now does the same through
`intern_attr_name` (falling back to a copy when there is no dict). `free_prop`
already guards dict-owned names via `dict_owns_str`, so teardown does not free
the shared entry.

This also brings `xmlNewNsProp`/`xmlSetNsProp` in line with the existing
`xmlNewProp`/`xmlNewDocProp` paths, which already interned.

Fixes `doc/objectify.txt` (288/288) and `test_pytype_deannotate`.

### 2. XInclude did not apply the `xml:base` fixup

`xmlXIncludeProcessTree` includes the referenced document element but never
told the copy where it came from, so an included `<a/>` lost the
`xml:base="doc/test.xml"` upstream adds. Relative references inside included
content then resolved against the wrong base.

Port `xmlXIncludeBaseFixup` (xinclude.c): the fixup is armed unless
`XML_PARSE_NOBASEFIX` is set on the process flags or on the including document;
`target_base` is the base of the xi:include node. After the deep copy, compare
the source root's base (`xmlNodeGetBaseSafe` on the included document) with
`target_base`; when they differ, compute a relative URI
(`xmlBuildRelativeURI`, or the raw base for URIs past `XML_MAX_URI_LENGTH`) and
set `xml:base` on the copy when the result contains a slash. Otherwise any
existing `xml:base` on the copy is removed (`xmlUnsetNsProp` on the XML
namespace).

Fixes `doc/api.txt` (92/92).

## Evidence

* Full suite: **19 raw unique failing ids**, down from 22 — **3 fixed,
  0 regressions**:
  * `doc/objectify.txt` (288/288 doctest examples)
  * `doc/api.txt` (92/92 doctest examples)
  * `test_pytype_deannotate`
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: 1250 passed / 0 failed.
* `failing-ids.txt` is the raw unique id set; the filtered delta against the
  pre-slice baseline is exactly the three ids above.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
