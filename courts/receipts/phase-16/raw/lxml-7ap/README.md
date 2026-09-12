# lxml court 7ap — DTD validation and DTD API parity

`candidate_sha=bdb8ed79` · oracle libxml2 2.15.3 · lxml 6.1.2 · 2007 tests

## Result

```
previous (d809cdc1 / lxml-7ak)   raw 64 ids, 62 after removing the flaky group
now (bdb8ed79 / lxml-7ap)        raw 58 ids, 55 after removing the flaky group
fixed                            7
new                              0
PHP six-gate                     1250 passed / 0 failed
cargo test --lib                 1325 passed / 0 failed / 3 ignored
```

`test_dtd.py` is now fully green (0 failed / 38 passed).

## 1 — xmlCopyDtd duplicated element→attribute links

`xmlCopyElement` copied the element's attribute declaration list. libxml2
deliberately leaves it NULL (the attribute declarations are copied separately by
the attribute table) and lxml's `_copyDtd` rebuilds the links by walking the
copied DTD's `XML_ATTRIBUTE_DECL` children. Because we copied them too, lxml's
rebuild appended every attribute a second time:

```
C probe:  xmlCopyDtd -> element 'a'.attributes
oracle    0
candidate 2   (then lxml's _linkDtdAttribute appended the 2 children -> 4)
```

`copy_element` now leaves `attributes` NULL, matching the oracle.

## 2 — xmlValidateDtd did not substitute the passed DTD

`etree.DTD(path).validate(root)` reported "No declaration for element a/b"
because the walk used the document's own (empty) subsets. Upstream
`xmlValidateDtd` (valid.c) saves `intSubset`/`extSubset`, substitutes the passed
DTD as the external subset, clears `doc->ids`/`doc->refs`, runs `xmlValidateRoot`
then `xmlValidateElement` + `xmlValidateDocumentFinal`, and restores.

The candidate now does exactly that. `xmlValidateRoot` also had to change: it
consulted the external subset for the DOCTYPE-name check, but upstream only
checks `doc->intSubset` (and only when named), so during substitution the check
is skipped. Without that, the new path produced a spurious
`Root element 'b' does not match DTD root 'none'`.

## 3 — ID registration mixed two payload types (segfault)

`validate_element` (the recursive walker) called a candidate-only `validate_id`
that stored the element **node** in `doc->ids`, while `add_id` and the table
deallocator store an `_xmlID`. With DTD substitution now reaching the walk,
`free_id_table` interpreted the node as an `_xmlID` and crashed:

```
#4 free_id (id=0x322aec80) at validation/mod.rs:2464   free(0x1)
#7 free_id_table ...
#8 validate_dtd ...
```

`validate_element` now registers ID attributes with `add_id`, matching upstream
`xmlValidateOneAttribute`. That both fixes the crash and yields the oracle's
diagnostic:

```
oracle    ID id1 already defined
candidate ID id1 already defined
```

## 4 — a broken content model compiled

`<!ELEMENT b HONKEY>` was silently dropped by `parse_element_decl`.
Upstream `xmlParseElementDecl` raises `XML_ERR_ELEMCONTENT_NOT_STARTED`
("xmlParseElementDecl: 'EMPTY', 'ANY' or '(' expected"). It is now a fatal
parser error.

## 5 — xmlIOParseDTD returned an empty DTD on failure

lxml parses `BytesIO` DTDs through `xmlIOParseDTD`, whose `parse_dtd_text`
fallback returned an empty DTD named "none" when the declarations failed to
parse. Upstream returns NULL and lxml raises `DTDParseError`. The fallback is
gone; a failed parse returns NULL.

A C probe confirms `xmlParseDTD(NULL, "broken.dtd")` returns NULL in both.
