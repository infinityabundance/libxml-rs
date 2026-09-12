# lxml court 7as — RELAX NG missing-start rejection

`candidate_sha=742a6f99` · oracle libxml2 2.15.3 · lxml 6.1.2 · 2007 tests

## Result

```
previous (24ead03b / lxml-7ar)   52 raw ids, 49 excluding the flaky group
now                              51 raw ids, 48 excluding the flaky group
fixed                             1
new                               0
PHP six-gate                     1250 passed / 0 failed
cargo test --lib                 1325 passed / 0 failed / 3 ignored
```

## The fix

`<grammar xmlns="http://relaxng.org/ns/structure/1.0"/>` (a top-level grammar
with no `<start>` pattern) compiled and validated everything. Upstream
`relaxng.c xmlRelaxNGParseGrammar` treats it as invalid and `xmlRelaxNGParse`
returns NULL, so lxml raises `RelaxNGParseError`.

`rng_parse_doc` now marks the schema fatal (`Element grammar: Missing start
element`) when the top-level grammar has no start pattern; `xmlRelaxNGParse`
already turns a fatal schema into NULL.

## Remaining in test_relaxng.py

`test_relaxng_generic_error` — the schema uses `<data type="IDREF"/>` and the
invalid instance references an undeclared ID. The validator does not yet
resolve datatype-library ID/IDREF references.
