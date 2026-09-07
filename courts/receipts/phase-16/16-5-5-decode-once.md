# Phase 16.5.5 — decode-once (no character decoded twice)

Commit: (this commit)
Date: 2026-09-07
Phase: 16.5.5

## Change

Per-char scanner loops peeked a character (`peek_char` → full UTF-8 decode)
and then consumed it with `read_char` (a SECOND full decode of the same
character). The advance logic never needed the decoded character at all —
`advance_past_char` is source-byte driven (the tab and regular-character
branches both advanced the column by 1), so its `c: char` parameter was
dead weight.

- `InputBuffer::advance_past_char` is now parameterless (source-byte only);
  `read_char` = decode once + advance.
- New `InputBuffer::consume_peeked()` / `InputStack::consume_peeked()`:
  advance past the character the caller just peeked WITHOUT decoding —
  position/line/col semantics byte-for-byte identical to `read_char`
  (`debug_assert` guards misuse).
- Converted the per-char hot loops to peek-then-`consume_peeked`:
  `skip_whitespace`, `scan_characters` (clean/CR/invalid-char arms),
  `scan_name`, `scan_attr_value_inner` (value chars, `&name;` scan, quote
  close). Structural one-shot consumes (markup bytes, terminators) keep
  `read_char` — the double decode only mattered per iteration.

Error offsets are untouched (position math unchanged); EOL substitution,
CRLF-pair consumption and line/col tracking are identical (verified by the
unit suite, the frozen-oracle A/B probes and the CLI court).

## Measurement

Criterion `parse` microbench vs the pre-change database (clean machine,
second run): −0.2% (283 B, within noise), +0.6% (2.9 KB, within noise),
−2.5% (31 KB), **−7.6% (328 KB)**. The element-heavy doc's per-char name and
text scans now decode once. No regressions on any size (an apparent +6% on
the first run was machine load — load avg 1.5+ — and disappeared on rerun).

## Gates

- `cargo test --lib`: 1264 pass / 0 fail.
- Oracle A/B (xmllint + node probe): text/CRLF/attr/doctype/PI/comment
  content byte-identical (only the documented pre-existing gaps remain:
  empty-comment serialization, double-hyphen preview, INT_SUBSET 118).
- PHP six-gate: 1250 passed / 0 failures.
- ASan fuzz parse: 391k runs clean.
