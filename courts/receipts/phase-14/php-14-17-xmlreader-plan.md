# Phase 14.17 — xmlreader cluster plan (26 → ~18 target)

Baseline: HEAD 6ad0e88c, xpe-six32.log = 26 failures
(dom 10 | xsl 7 | xmlreader 8 | xmlwriter 1 | simplexml 0)

## Why xmlreader next
8 failures with 6 bounded root causes, all in src/xml/reader/mod.rs +
schema/relaxng core. Fastest verified fan-in left; xsl items (extension
dispatch, template-depth, param expanded-names) follow in 14.18.

## RC-1 reader pre-read cursor semantics  (fromStream/fromString_custom_constructor)
Oracle pre-read (node==NULL): AttributeCount=0; IsEmptyElement=-1;
ConstBaseUri=NULL. Candidate: AttributeCount=-1; IsEmptyElement=0;
ConstBaseUri=URL(CWD).
- upstream xmlreader.c: AttributeCount 2884 (node==NULL -> 0);
  IsEmptyElement 2995 (node==NULL -> -1); ConstBaseUri 3348 (node==NULL -> NULL).
Fix the three Rust accessors (exports in src/xml/reader/mod.rs).

## RC-2 xmlTextReaderSetSchema must not fail on schemas it can compile with
warnings; errors surface at read() time (013 + bug73053). Investigate why
candidate schema parse fails on 013.xsd/bug73053.xsd. "Schema contains errors".

## RC-3 bug64230 error message garbage -> error buffer NUL/lifetime in the
reader error channel.

## RC-4 gh19098 namespaces lost through reader expand/next("name").

## RC-5 007 relative relaxNG resolution.

## RC-6 fromStream_broken_stream: reader-from-IO on a stream that errors
mid-way must still deliver element/comment then end.

## Gate
cand-six-gate.sh -> name-level diff vs xpe-six32.log (NEW_ONLY empty),
count drop by >=8. Receipt php-14-17-xmlreader-...
