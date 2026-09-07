# Phase 16.6 — scalar engine: bulk name + whitespace scanning

Commit: (this commit)
Date: 2026-09-07
Phase: 16.6 (establish the best possible scalar engine — the fallback /
correctness reference for the §16.7 vector paths)

## Where the scalar work stands

§16.6 requires a formidable scalar baseline BEFORE any SIMD. The §16.5.x
work already established much of it (span tokens with bulk segment delivery,
decode-once consumes, printable-ASCII text fast path, slot-address
allocator bridge, O(1) tree merge). This commit adds the two remaining
tokenizer hot-loop scalar scans:

### 1. `scan_name` ASCII bulk prefix
Names are overwhelmingly ASCII. When the first byte is an ASCII
NameStartChar (letter / `_` / `:`), the ASCII continuation run
(`[A-Za-z0-9._:+-]`, with `+` per libxml2's lenient IS_CHAR rule) is
consumed in one tight byte pass (`ascii_name_byte` take_while — no per-char
UTF-8 decode) and copied with one `extend_from_slice`. Names starting with a
multi-byte char, or ASCII prefixes followed by a multi-byte continuation,
fall back to the unchanged per-char loop for the remainder (`first` already
false) — semantics identical. Consumed bytes are ASCII name chars, so
`skip_linebreak_free` (col += n, no line tracking needed) is valid.

### 2. Whitespace runs are bulk-skipped
`skip_whitespace` (prolog / between attributes) was a per-char
peek+decode+consume loop. New `InputBuffer::skip_ascii_whitespace`
consumes an ASCII-whitespace run (space, tab, CR, LF, form feed) with
line/column semantics byte-for-byte identical to the per-char loop: a CRLF
pair counts as ONE line break (the `advance_past_char` CR branch consumes
the LF), every other byte advances the column by one — no decoding at all.
`InputStack::skip_ascii_whitespace` pops exhausted entity inputs across the
stack exactly like the old peek+read loop.

## Measurement (release xmllint / criterion, quiet machine)

- Name bulk scan (criterion parse, element-heavy): −4.3 % … −11.6 % across
  the four sizes (largest size −11.6 %).
- + whitespace bulk scan (criterion parse, measurement-time 5, rerun on a
  quiet machine): −2.4 % (283 B), within noise (2.9 KB), −0.5 % (31 KB),
  −4.9 % (328 KB). (A first run showed a spurious +12 % at the largest size
  — load-average noise; the rerun disproved it.)
- Element-heavy 30 MB doc (400k items): 0.669 s → 0.60 s (frozen oracle:
  0.424 s; gap to close in §16.7 + the C-side element machinery).
- Cumulative §16.6.x parse gain on the criterion microbench since the
  §16.5.3 baseline: 7.18 ms → 6.19 ms at 328 KB (−14 %); 7.05 µs → 5.86 µs
  at 283 B (−17 %).

## Gates

- `cargo test --lib`: 1264 pass / 0 fail.
- CLI xmllint differential: 46/48 byte-identical (2 pre-existing fails).
- Oracle A/B (line/col-sensitive CRLF + doctype probes): byte-identical.
- PHP six-gate: 1250 passed / 0 failures.
