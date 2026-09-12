# lxml court 7ai — XSD model-group occurs + structured validation errors

`candidate_sha=81e5f013` · oracle libxml2 2.15.3 · lxml 6.1.2 · 2007 tests

## Result

```
previous (4960a85b / lxml-7ah)   79 unique failing ids
now (81e5f013 / lxml-7ai)        72 unique failing ids
fixed                             7
new                               0
PHP six-gate                     1250 passed / 0 failed
cargo test --lib                 1325 passed / 0 failed / 3 ignored
```

`test_xmlschema.py` went from 10 failed / 21 passed to 3 failed / 28 passed.

## 1 — a model group's own minOccurs/maxOccurs was ignored

The four attribute-parity tests all use

```xml
<xsd:complexType name="AType">
  <xsd:sequence minOccurs="4" maxOccurs="4">
    <xsd:element name="b" type="BType" />   <!-- part maxOccurs defaults to 1 -->
  </xsd:sequence>
</xsd:complexType>
```

The `Sequence` arm of `xsd_validate_model_group` matched each part exactly
`part.max_occurs` times (here 1) and never repeated the group, so four `<b/>`
children were rejected:

```
oracle     valid=True  attrs ['ho','hey','ho','hey']
candidate  valid=False "Unexpected element 'b' in sequence"
```

The arm now repeats the whole group `gmin..gmax` times, matching each part per
its own bounds within each repetition. The default `1..1` still reduces to a
single pass.

Error selection was pinned against the oracle rather than guessed:

| shape | oracle message |
|---|---|
| mid-sequence mismatch | `Element 'X': This element is not expected. Expected is ( Y ).` |
| trailing extras | `Element 'X': This element is not expected.` |
| missing content | `Element 'A': Missing child element(s). Expected is ( B ).` |
| missing, multiple reachable | `... Expected is one of ( B, C ).` |

The trailing-extra case previously emitted `Unexpected element 'X' in
sequence`, which is not an upstream message at all; it is now the bare
`This element is not expected.` form. The missing-child case previously
reported once per unsatisfied part (two diagnostics for a two-part sequence);
it now reports once, listing only the particles still reachable — the first
required part when nothing matched, otherwise the failed part against the
saturated-prefix rule.

## 2 — validation diagnostics had no code and no node

`XsdValidCtxt.errors` was a `Vec<String>`; `dispatch_valid_errors` set
`e.code = 0` and left `e.node` NULL. Consequences:

- `test_xmlschema_error_log` — `error_log.filter_types(SCHEMAV_ELEMENT_CONTENT)`
  was empty.
- `test_xmlschema_error_log_path` — `error_log[0].path` was `None`.

`errors` is now `Vec<XsdValidationError { code, node, message }>`; every
validation site records the offending node (the child for content errors, the
parent for missing-child, the element for attribute errors), and
`dispatch_valid_errors` sets `e.code = 1871` (`XML_SCHEMAV_ELEMENT_CONTENT`,
the value lxml reports) and `e.node`. Oracle probe:

```
val err: 1871 SCHEMAV_ELEMENT_CONTENT "Element 'c': This element is not
          expected. Expected is ( b )." path= /a/c
```

## 3 — schema components outside the XSD namespace compiled

`test_xmlschema_invalid_schema1` uses a bare `<element name="a">` (no `xsd:`
prefix). The component parser matched by local name, so it compiled. Upstream
validates the `<schema>` content model and rejects any component not in
`http://www.w3.org/2001/XMLSchema`:

```
oracle  XMLSchemaParseError: Element '{http://www.w3.org/2001/XMLSchema}schema':
        The content is not valid. Expected is ((include | import | redefine |
        annotation)*, (((simpleType | complexType | group | attributeGroup) |
        element | attribute | notation), annotation*)*)., line 2
```

`xsd_parse_schema_doc` now checks every element child of `<schema>` and returns
an error when its namespace is not the XSD namespace, so `xmlSchemaParse`
returns NULL and lxml raises `XMLSchemaParseError`. A foreign-namespace
component fails the same way (verified).

## Verification

`target/scratch/probe_xsd*.py` compare candidate against oracle for:
exact-count valid shapes, under/over count, multi-part sequences, repeated
groups, part-mismatch messages, and unqualified/foreign-namespace components.
All match.

## Remaining test_xmlschema failures

`test_xmlschema_parse`, `test_xmlschema_stringio`, `test_xmlschema_iterparse_fail`
need `xmlSchemaSAXPlug` to intercept SAX events during parse. lxml calls
`xmlSchemaSAXPlug(valid_ctxt, &ctxt.sax, &ctxt.userData)` before parsing and
decides afterwards via `xmlSchemaIsValid(valid_ctxt)` (`parser.pxi`
`_handleParseResult`, line 723). The candidate's `xmlSchemaSAXPlug` is a
documented pass-through stub: it leaves `*sax`/`*user_data` untouched, so no
events reach the validator and `xmlSchemaIsValid` reports the previous run's
vacuous state. Implementing it needs the plug to feed a schema validation
context from SAX events (upstream's own validator state, or a shadow tree).
