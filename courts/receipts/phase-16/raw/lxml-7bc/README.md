# lxml court 7bc — RELAX NG ID/IDREF registration and the final IDREF check

## Root causes fixed

### 1. `validate_document_final` never reported an unresolved IDREF

`xmlValidateDocumentFinal` (valid.c) walks `doc->refs` — a value-keyed hash of
`xmlRef` lists — and reports every reference whose value has no entry in
`doc->ids`. Our callback read the *second* hash key (`name2`) as the reference
value, but `xmlAddRef` stores a single key (`name`), so `name2` is always NULL
and the callback returned immediately. DTD IDREF validation was therefore a
silent no-op.

The callback now walks the `xmlRef` list payload (upstream `xmlValidateRef`)
and reports the attribute name/line from the `xmlRef`, with the upstream
messages

```
IDREF attribute <name> references an unknown ID "<value>"
IDREFS attribute <name> references an unknown ID "<value>"
```

(the IDREFS form selected by the attribute's `atype`).

### 2. RELAX NG did not register ID/IDREF attributes

Upstream's XML Schema datatype validation registers an `ID` attribute with
`xmlAddIDSafe` and `IDREF`/`IDREFS` with `xmlAddRef` (xmlschemastypes.c
`xmlSchemaValAtomicType`). libxml2's RELAX NG validator then runs
`xmlValidateDocumentFinal` when the schema declared an IDREF data type
(`xmlRelaxNGValidateDoc`). Our validator neither marked the schema
(`idref`) nor registered the attributes, so `test_relaxng_generic_error` never
saw the dangling reference.

* `RelaxNgSchema` gains `idref`, armed by `rng_parse_data_pattern` for an
  `IDREF`/`IDREFS` data pattern (XML Schema datatype library).
* Attribute matches register their value in the document tables through
  `rng_register_attr_id_ref` (deduplicated per attribute, because the matcher
  can revisit an attribute across alternate match states).
* `xmlRelaxNGValidateDoc` runs the final check after a successful match and
  folds its diagnostics into the RELAX NG error list.

Fixes `test_relaxng_generic_error`.

## Evidence

* Full suite: **11 raw unique failing ids**, down from 12 — **1 fixed,
  0 regressions**:
  * `test_relaxng_generic_error`
* `test_relaxng.py` 13 passed / 3 skipped / 0 failed.
* `cargo test --lib`: 1325 passed / 0 failed / 3 ignored.
* `cargo fmt --check`: clean.
* PHP six-gate: 1250 passed / 0 failed.
* `failing-ids.txt` is the raw unique id set; the filtered delta against the
  pre-slice baseline is exactly the one id above.

GitHub still carries no status checks or workflow runs; these are local and
committed results.
