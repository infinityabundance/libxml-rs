# Phase 16.5.4 — duplicate-attribute detection without per-name clones

Commit: (this commit)
Date: 2026-09-07
Phase: 16.5.4

## Change

`scan_start_tag` detected duplicate attribute names by cloning every
attribute name into a `Vec<Vec<u8>>` "seen" list on EVERY start tag (one
heap allocation + copy per attribute, freed after the scan). The names are
already owned inside the token's `attributes` Vec, so:

- ≤ 8 attributes (the real-world case): nested in-place slice compares —
  zero allocations, no hashing overhead.
- > 8 attributes (pathological tags): a `HashSet<&[u8]>` keeps the scan
  O(k) instead of O(k²).

Error semantics are unchanged: the first attribute (in document order) that
duplicates an earlier one raises "Attribute %s redefined" (42) with str1 =
name at the tag-end position. Duplicate-attribute oracle parity verified on
`<a a="1" a="2" b="3"/>` (identical diagnostic; only the stderr filename
differs).

## Measurement

Criterion `parse` microbench (element-heavy doc, one attribute per element —
so the change removes one alloc/free pair per element):
- run 1: −2.1% (283 B), +0.6%/within-noise (2.9 KB), +2.2% (31 KB — load
  noise), within-noise (328 KB).
- run 2 (clean machine): **−0.5% / −0.7% / −4.4% (31 KB) / −6.3% (328 KB)** —
  the larger sizes improved as allocator traffic dropped; no regression.

## Gates

`cargo test --lib`: 1264 pass / 0 fail. Duplicate-attr CLI A/B identical to
the frozen oracle.
