# Phase 16.5.6 — contiguous text scanning (bulk printable-ASCII fast path)

Commit: (this commit)
Date: 2026-09-07
Phase: 16.5.6

## Change

`scan_characters` decoded and decision-checked ordinary text one Unicode
scalar at a time (peek → validate XML Char → consume) even for bytes that
can never need a decision. A bulk fast path now runs before the per-char
loop: bytes in `0x20..=0x7E` excluding `<`, `&`, `]` are scanned as a
contiguous run (a tight scalar byte loop — no decoding, no function calls;
the compiler vectorizes it, and the SIMD structural scanner of §16.7 builds
on the same seam). Every such byte is a valid XML Char that decodes to
itself, is never CR/LF (no §2.11 EOL substitution, no line/col change), and
never `]` (the `]]>` lookahead) — so the run only extends the pending text
segment and bumps the column once.

Guards: the fast path yields to the per-char loop whenever the re-parse
split boundary (`split_chars_at`) is active, because only that state can
legally break a run mid-bytes. Entity-content inputs take the same path —
an exhausted entity buffer is flushed by the existing pre-pop logic exactly
as before.

`InputBuffer::skip_linebreak_free` / `InputStack::skip_linebreak_free`
advance a proven line-break-free run (pos + one column per byte).

## Measurement

Release xmllint `--noout`, 35 MB text-heavy document (400k `<p>` runs):
**0.429 s → 0.179 s** (2.4×), three runs stable. Frozen oracle: 0.100 s —
the residual gap is §16.6 (scalar engine) / §16.7 (SIMD) territory.

## Correctness / gates

- The change is behavior-neutral: the tree produced for `/tmp/bigtext.xml`
  is node-probe byte-identical to the frozen oracle, and the node probes for
  the CRLF/doctype/entity corpus all still MATCH.
- `cargo test --lib`: 1264 pass / 0 fail.
- PHP six-gate: 1250 passed / 0 failures.
- ASan fuzz parse running clean (300 s window).
- Note (pre-existing, unchanged): `xmllint --format` output for documents
  whose parents hold whitespace-only text nodes between element children
  differs from the oracle's formatter indentation (candidate emits the
  whitespace text verbatim, oracle re-indents). The trees are identical;
  this is a serializer-format-mode behavior gap tracked separately.
