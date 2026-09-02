# SP-14.3.1-2 — undeclared-entity severity (R-14.3-UNDECLARED-ENTITY-SEVERITY)

Closed 2026-09-02. ext/xml: **20 -> 18 failed** (xml004 + xml_closures_001 flip
PASS, zero sibling regressions; full-suite log `/out/sp1-xml3.log` = 67 run /
38 pass / 18 fail / 11 skip).

## Root cause

`xml004.phpt`/`xml_closures_001.phpt` parse `ext/xml/tests/xmltest.xml`:

```xml
<!DOCTYPE phptest SYSTEM "notfound.dtd" [
<!ENTITY % incent SYSTEM "inc.ent">
%incent;
]>
...
<elem3>
 &included-entity;
 <elem4>...
```

The candidate's SAX push path (PHP expat-compat layer: `xmlCtxtUseOptions(
OLDSAX | NOENT)`, SAX `getEntity` = PHP `get_entity`) raised **every**
undeclared general entity reference as `XML_ERR_UNDECLARED_ENTITY` (26) at
FATAL level in `parse_reference`, aborting the parse and dropping `<elem4>`
… `</root>`.

Upstream (`parser.c` 2.15.0 `xmlHandleUndeclaredEntity`) gates the fatal
raise:

```c
if ((ctxt->standalone == 1) ||
    ((ctxt->hasExternalSubset == 0) && (ctxt->hasPErefs == 0)))   -> FATAL 26
else if (ctxt->validate)                                          -> validity err 26
else if ((ctxt->loadsubset & ~XML_SKIP_IDS) ||
         (ctxt->replaceEntities && !(options & XML_PARSE_NO_XXE))) -> xmlErrMsgStr 27 ERROR
else                                                              -> xmlWarningMsg 27 WARNING
```

`xmltest.xml` has `SYSTEM "notfound.dtd"` (hasExternalSubset = 1) and
`%incent;` (hasPErefs = 1), so the reference is **non-fatal**; the parse
continues and emits the remaining events. The candidate never tracked either
flag, and its `xmlCtxtUseOptions` was an options-only stub, so
`ctxt->replaceEntities` stayed 0 under PHP's `NOENT` compat path.

## Fixes (Rust engine layer, mirroring upstream)

1. `src/xml/parser/state.rs` `parse_dtd`: DOCTYPE with a SYSTEM/PUBLIC id now
   sets `ctxt->hasExternalSubset = 1` (upstream `xmlParseDocTypeDecl`).
2. `src/xml/parser/state.rs` `parse_internal_subset`: a well-formed `%Name;`
   reference in the subset sets `ctxt->hasPErefs = 1` before any resolution
   (upstream `xmlParsePERefInternal` line 7618).
3. `src/xml/parser/state.rs` `parse_reference` undeclared branch: mirrors
   `xmlHandleUndeclaredEntity` branch-for-branch (fatal 26 | DTDVALID
   validity error 26/XML_FROM_DTD | `xmlErrMsgStr` 27 ERROR | `xmlWarningMsg`
   27 WARNING), then `ctxt->valid = 0`, and fires the SAX reference event only
   when `replaceEntities == 0` (upstream `xmlParseReference` ent==NULL
   continuation).
4. `src/abi/exports_xml2.rs` `xmlCtxtUseOptions`: replaces the options-only
   stub with the upstream `xmlCtxtUseOptions -> xmlCtxtSetOptionsInternal`
   behavior — keep-mask merge plus historical member derivation
   (recovery/replaceEntities/loadsubset/validate/pedantic/keepBlanks/
   dictNames) — and returns the unhandled option bits like upstream.

## Oracle-pinned expectations

System 2.15.3 `xmlCtxtReadMemory` (options 0 / NOENT):

| doc | opts | result |
| --- | --- | --- |
| `<root>a&nope;b</root>` | 0 | fatal, wellFormed=0, errNo=26, no doc |
| `<!DOCTYPE root [<!ENTITY e "E">]><root>a&nope;b</root>` | 0 | fatal, errNo=26, no doc |
| `<!DOCTYPE root SYSTEM "nope.dtd">…` | 0 | warning 27, errNo=0, doc yes |
| `<!DOCTYPE root SYSTEM "nope.dtd">…` | NOENT | error 27, errNo=27, doc yes |
| `<!DOCTYPE root [<!ENTITY % p SYSTEM "x"> %p;]>…` | 0/NOENT | non-fatal, doc yes |

PHP compat (expat layer) on the php-oracle container ends the same document
with `xml_get_error_code() == 27` ("Undeclared entity warning") and emits the
full element sequence — the xml004 contract.

## Guards

- `src/xml/parser/tests.rs`: `test_undeclared_entity_fatal_without_extsubset_or_perefs`,
  `test_undeclared_entity_nonfatal_with_extsubset_or_perefs`,
  `test_undeclared_entity_noent_nonfatal_error_27` (all oracle-pinned above).
- `courts/suites/phase14/consumers/undecl-entity-probe.c` — reusable C probe.
- `cargo test --lib` 1202 pass / 0 fail / 1 ignored; clippy (no new warnings)
  and `cargo fmt --check` clean.
