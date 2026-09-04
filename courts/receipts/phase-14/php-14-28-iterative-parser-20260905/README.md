# Phase 14.28 — iterative element driver (R-000199 FIXED): deep documents parse crash-free

Gates: NTS + ZTS six-extension file-list = **1290 / 1250 passed / 40 skipped /
0 failed** each (`/out/nts-cand-six-1428.log`, `/out/zts-cand-six-1428.log`).
`cargo test --lib` 1247 pass; cargo fmt clean; valgrind 0 errors on a
depth-20000 parse.

## The defect

The candidate parsed element content by RECURSIVE descent
(`parse_element` → `parse_element_content` → `parse_element` per nesting
level, ~3.3KB/level at the −O0 dev profile). The oracle drives the same
work through the ITERATIVE `xmlParseTryOrFinish` state machine, so it parses
depth-20000 documents on a 1 MB stack while the candidate SEGFAULTed at
depth 4000 on the 8 MB php stack:

```
ext/xml xml_parse depth: candidate 2000 ok, 3000 ok, 4000 SEGFAULT (rc 139)
                        oracle   20000 ok (unbounded; no cap on the SAX path)
```

The 2.15 depth LIMITS (256 / 2048 with XML_PARSE_HUGE, "Excessive depth in
document") live in the TREE builder (`nodePush`/`xmlSAX2StartElementNs`) —
mirrored in `sax/default.rs` — so DOM/tree consumers hit the graceful cap at
2048 on BOTH sides, while SAX-only consumers (php ext/xml `xml_parse`,
`xml_parse_into_struct`) are unbounded and must not crash.

## The fix

`src/xml/parser/state.rs`: `parse_element` is now an ITERATIVE driver.

- The former recursion levels are an EXPLICIT heap stack of open-element
  frames (each an `OpenElement` with `name`/`open_line`/`ns_scope_mark`).
- A nested start tag runs `parse_element_start` for the child, pushes the
  current frame, and makes the child current — no C-stack growth.
- An end tag (matched or stray-mismatch) closes the current element via the
  shared `close_open_element` (SAX end event, ns-scope truncate, name pop)
  and resumes the parent's loop (`close_element_and_resume`).
- The former `parse_element_content` body was folded into the driver
  VERBATIM; only the end-tag/start-tag arms and the `name`/`open_line`
  references were rewritten. Every other branch (characters, comment, PI,
  CDATA, references, EOF, DOCTYPE, recovery default) and every error path's
  `pop_name` behavior is byte-for-byte the same code, so the SAX/name/
  namespace push-pop order is unchanged at every depth.

`parse_element_content` was deleted; the only remaining mentions are
historical doc comments.

## Verification (candidate == oracle)

`deep-doc-parity-probe.php` (this dir) run against the oracle and the
candidate:

| case | oracle | candidate |
|---|---|---|
| ext/xml SAX, depth 5000 | ok=1, events=10002, max=5001, INNER:A | identical |
| DOM tree, depth 2000 (HUGE) | ok, saveXML 14027 bytes | identical |
| DOM tree, depth 2048 (HUGE) | ok=false (tree cap) | identical |
| DOM tree, depth 2049 (HUGE) | ok=false (tree cap) | identical |

Depth sweep (ext/xml): candidate 4000/20000/100000 all parse (rc=0);
valgrind 0 errors at depth 20000. DOM tree-building behavior is unchanged
(≤2048 parses byte-identically; the nodePush cap error at 2048+ matches the
oracle exactly, as before).

Probes: `parse-depth-probe.php`, `depth-sweep-probe.php`,
`depth-cap-parity-probe.php`, `deep-doc-parity-probe.php`.
