# Phase 16.5.3 — token span architecture (owned-or-span text tokens)

Commit: (next commit)
Date: 2026-09-07
Phase: 16.5.3 (tokenizer must not allocate merely to describe bytes that
already exist in the input)

## What changed

### `XmlText { Owned | Span }` content tokens
New `XmlText` enum in `src/xml/parser/tokenizer.rs`:
- `XmlText::Owned(Vec<u8>)` — patched bytes (or a clean entity-content run
  copied once).
- `XmlText::Span { start, end }` — byte offsets into the BASE input's data;
  zero tokenizer allocation.

`XmlToken::Characters`, `Comment.data` and `Cdata.data` now carry `XmlText`.
The body scanners (`scan_characters`, `scan_comment_body`, `scan_cdata_body`)
use a **pending-segment model**: while the delivered bytes equal the source
bytes since `seg_start`, nothing is materialized; delivery happens ONLY as
bulk source-range flushes at (a) a patch point, (b) an entity-input auto-pop,
and (c) run finish. Patch points (invalid XML char dropped, invalid UTF-8 →
U+FFFD, source CR → LF by §2.11 EOL substitution, the dropped `--` of the
comment double-hyphen WFC error) flush the pending segment and append the
patched bytes explicitly.

Span validity contract (documented on `XmlText`): spans exist only for
base-input runs; tokens are consumed synchronously in the same loop
iteration that produced them (only StartTag is ever pushed back); the base
buffer's data cannot mutate between production and consumption. Entity-
content runs never span (their buffer is popped+dropped at the boundary);
comments/CDATA inside entity inputs use a legacy per-char owned loop.

### Zero-copy SAX dispatch
`sax_characters_text` / `sax_cdata` resolve a payload and dispatch
`(ch, len)` directly — a pure base run hands the C handler a pointer INTO
the input data, exactly like upstream `xmlParseCharDataComplex` handing over
`input->cur + len` (no NUL-terminated copy was ever part of the SAX
`characters`/`cdataBlock` contract). The NOBLANKS whitespace gate now scans
the run only when `keepBlanks == 0` (the default path must not pay a
per-byte scan — criterion-verified). `sax_comment` still builds the
NUL-terminated string the comment callback requires, but from the resolved
payload (no tokenizer Vec first).

`InputStack::base_input_range` + `InputBuffer::raw_range` expose the ranges;
`XmlTokenizer::text_bytes` resolves payloads for the parser and tests.

## Experiment A (mandated before/after: allocations, cycles, throughput)

`examples/alloc_probe` (`<r>xxxx…</r>`, single huge text run), release build:

| input | path | before (owned tokens) | after (spans) |
|---|---|---|---|
| 1 MiB | xmlReadMemory | 1,051,095 B / 28 allocs | **2,519 B / 10 allocs** |
| 2 MiB | xmlReadMemory | 2,099,671 B / 29 | **2,519 B / 10** |
| 4 MiB | xmlReadMemory | 4,196,823 B / 30 | **2,519 B / 10** |
| 1 MiB | xmlCtxtReadMemory | 2,099,668 B / 29 | 1,051,092 B / 11 (the 1 MiB input copy upstream requires) |
| 4 MiB | xmlCtxtReadMemory | 8,391,124 B / 31 | 4,196,820 B / 11 |

Per-parse Rust allocation is now **O(1) in text volume**: at 4 MiB input the
borrowed path allocates 2.5 KB vs 4.2 MB before (~1665× fewer bytes; the
two per-run allocations — tokenizer Vec + C-string — are gone).

Criterion `parse` microbench (element-heavy, 283 B – 328 KB docs): no
statistically significant change after removing an accidental unconditional
per-run whitespace scan (all sizes within noise; the early version's
+1.9%@328 KB regression is gone).

Real-document throughput (release xmllint, `--noout`): a 35 MB text-heavy
doc that previously did not complete in 60 s (tree-merge quadratic, below)
now parses in 0.43 s (frozen oracle: 0.104 s — gap to close in §16.6/16.7).

## Bonus: pre-existing defects fixed while validating the span model

1. **O(N²) default-handler text merge** (`xml/sax/default.rs`): the default
   `characters` handler walked the parent's whole child list to find the
   last child on EVERY text event; a root with N element children plus
   inter-element whitespace was O(N²). Upstream is O(1) via `node->last`.
   Now uses `(*parent).last`. Measured: 100k-item doc 15.5 s → **0.111 s**.
   (Verified pre-existing at the 16.5.3-byte-model commit.)
2. **Infinite loop on a bare `&` in entity content**
   (`parse_entity_content`, state.rs): `&` with no following `;` stalled the
   scan forever (the literal-copy loop stops at `&`). The byte is now copied
   as literal text so the scan always progresses. (ASan-fuzz `timeout-`
   artifact `6abaef6c`.) The oracle rejects such a value at DECLARATION time
   ("EntityValue: '&' forbidden...") — that decl-time diagnostic gap is
   tracked separately (below); the hang is gone.
3. **`name_cstr` leak on early `Err` returns** in `parse_entity_content`
   (recursive parse failure / amplification check leaked the reference-name
   string; now freed on those paths). ASan leak artifact `0488847c`.

## Verification (all green)

- `cargo test --lib`: **1264 passed / 0 failed** (new
  `test_token_span_model` pins Span-vs-Owned; EOL/doctype regressions from
  the byte-model commit still pass).
- Oracle A/B (node-dump probe + xmllint `--format`/`--noent`): text, CRLF,
  CDATA, comments (incl. inside entity content), doctype, PI, entity
  expansion, NOENT, empty-comment-adjacent content — byte-identical except
  the two documented pre-existing gaps below.
- PHP six-gate: 1250 passed / **0 failures** (three runs).
- CLI xmllint differential: 46/48 byte-identical (the 2 failures are the
  pre-existing CLI-XMLLINT-0019/0024).
- ASan fuzz: parse +360 s clean on the final code (running), html/xpath
  clean; all prior leak + timeout artifacts fixed.
- lxml oracle/candidate smoke identical on CRLF/DTD/attr/PI content.

## Noted pre-existing gaps (unchanged, tracked separately)

- `<!---->` empty comment: candidate tree diverges from oracle at
  serialization (pre-existing at the byte-model commit).
- Double-hyphen comment error preview: candidate excludes previously-dropped
  `--` from the "Double hyphen within comment" message preview (oracle
  includes them) — pre-existing.
- `<!DOCTYPE a [x<]><a/>` INT_SUBSET "Content error" diagnostic (errNo 118)
  — pre-existing.
- Bare `&`/`<` inside entity literal values: no hang now (literal recovery),
  but the decl-time diagnostic parity (oracle rejects the declaration) is
  open.
- Undeclared `&e;` in attribute values (oracle: error + NULL doc) —
  pre-existing.
