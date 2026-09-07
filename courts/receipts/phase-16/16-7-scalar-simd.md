# Phase 16.7 — SIMD structural scanning: seal receipts

Commit: (this commit)
Date: 2026-09-07
Phase: 16.7 (SIMD structural front end) — scalar / AVX2 / AVX-512BW with
runtime dispatch, plus the §16.7.7 differential court that found and fixed
a release-only crash and five upstream-parity gaps.

## 1. The scanner (committed with the module)

`src/xml/parser/scan/{mod,scalar,x86}.rs` + tokenizer wiring (commit
08881f49): the printable-ASCII text-run classification (`0x20..=0x7E` minus
`<`, `&`, `]` — never CR/LF, always a valid XML Char decoding to itself) is
computed by one of three byte-identical backends:

- scalar — reference implementation (the §16.7.7 correctness reference);
- AVX2 — 32-byte lanes, unsigned range + structural-byte compares via
  min/max/movemask (§16.7.4);
- AVX-512BW — 64-byte lanes, native k-mask compares (§16.7.5).

Selection: cached once per process in a `OnceLock` — never CPUID inside a
scanner loop (§16.7.6). Private diagnostic override
`LIBXML_RS_SCAN_BACKEND=scalar|avx2|avx512|auto` (used by the differential
court and the fuzzers). `auto` resolves to AVX2 on capable CPUs; AVX-512 is
measured but not yet auto-selected (§16.7.5 policy decision recorded below).

## 2. §16.7.7 boundary + differential suites

- In-crate boundary test `backends_agree_on_boundaries`: every length
  0..=260 × every start offset with every interesting byte at every lane
  position + clean runs through 1000 B.
- In-crate PRNG differential `backends_agree_under_prng_fuzz`: 400 000
  deterministic xorshift64 buffers (lane-clustered lengths 0..4096, uniform
  bytes / printable text with sprinkled structural bytes / UTF-8 lookalikes /
  whitespace+control / pure-content / 2-byte alphabets / planted terminators)
  must classify identically across every supported backend, unaligned
  mid-buffer slices included.
- Parse-level court `courts/suites/phase16/` (new): 1116 generated
  adversarial documents (text runs at every length through the vector-lane
  zones, `<`/`&`/`]` planted at every lane position, `]]>` crossings, CR/LF
  in runs, truncated multi-byte UTF-8, nested markup/entity run ends, 4 MiB
  single-run and 2 MiB markup-heavy docs, malformed set) + 75 phase-14
  consumer fixtures, parsed by the node-dump probe compiled against the
  oracle and against the candidate under each forced backend, stdout/stderr
  kept separate.

Results (this commit, host = AMD Ryzen 7 9800X3D / Zen 5, docker oracle
libxml2 2.15.3):

```
corpus: 1191 documents
probe oracle rc=0 rows=22222 err=2655
probe scalar rc=0 rows=22222 err=2633
probe avx2   rc=0 rows=22222 err=2633
probe avx512 rc=0 rows=22222 err=2633
G1 PASS backend-invariance: scalar == avx2   (stdout+stderr byte-identical)
G1 PASS backend-invariance: scalar == avx512 (stdout+stderr byte-identical)
G2 PASS oracle == candidate(scalar): out byte-identical   (all 1191 parse trees)
G2 DIFF oracle vs candidate(scalar): err differs (174 candidate-only lines)
```

G1 — the §16.7 gate — PASSES: observable parser behavior (trees AND
diagnostics) is byte-identical across scalar/AVX2/AVX-512BW. G2 parse trees
are byte-identical to the oracle across the whole corpus. The 174 residual
stderr lines are pre-existing diagnostic-parity items outside the scan
surface (see §4).

Fuzz (§16.7.7): the parse fuzzer (40 692 seed corpus) was run with each
backend forced for 75 s: scalar 144 019 runs / avx2 151 507 / avx512
150 523 — zero crashes/artifacts on any backend (ASan-instrumented).

## 3. Bugs the court found and fixed (release-only, upstream-parity)

### 3.1 Release-only crash: free(stack pointer) on every XML_ERR_INVALID_ENCODING raise

The streamed generic-error trampolines `ch_call0/1/2`
(`src/xml/errors/mod.rs`) declared their call-target argument registers as
plain `in(...)`. The called handler is an arbitrary C function that
CLOBBERS the caller-saved argument registers; an undeclared input clobber
is UB, so LLVM was free to keep a stale input value live across the asm and
reuse it afterwards. In the `format_error_streamed` "Bytes:" hex loop
(encoding-error fragments) the compiler reused the stale `rsi` ("fmt")
value as the String buffer pointer for the per-iteration drop → `free()` of
a stack address → glibc "double free or corruption (out)" (rc=134) on EVERY
invalid-UTF-8 report (`<r>ab\xc3` etc.), release+LTO builds only.

Evidence trail: §16.7.7 court crash → valgrind-in-container ("Invalid free
... on thread 1's stack") → disassembly showed the stale-register reuse →
fix: every argument register is now `inlateout(...) => _`, so LLVM knows
the asm clobbers rdi/rsi/rdx/rcx/rax/r8-r11 (memory is an implicit clobber;
the asm is never `nomem`).

Fix verified: valgrind-clean on the crashing inputs; all 13 invalid-UTF-8
shapes byte-match the oracle; the same fix also resolved a silent
acceptance divergence (`a\xc3b` text previously parsed rc=0 with no
diagnostic — now byte-identical to the oracle).

This asm-contract violation dates to the Phase-11 streamed-error work and
is a latent class: the §16.7.7 court is the first suite that exercised
encoding-error raises en masse.

### 3.2 Incomplete-UTF-8-at-EOF vs mid-buffer encoding error (char data)

`xmlCurrentChar` (parserInternals.c 2.15) has two failure modes the
tokenizer conflated:
- incomplete_sequence (fewer bytes remain in the input than the char
  needs: avail < 2 for any byte ≥ 0x80, < 3 for a 3-byte lead, < 4 for a
  4-byte lead): `xmlParseCharDataComplex` raises a FATAL PARSER-domain
  XML_ERR_INVALID_CHAR "Incomplete UTF-8 sequence starting with %02X\n"
  PER offending byte (each consumed alone — a truncated 3-byte lead at EOF
  raises one error per remaining byte) and does NOT emit U+FFFD;
- encoding_error (enough bytes but not valid UTF-8): the I/O-domain
  XML_ERR_INVALID_ENCODING (81) "Invalid bytes in character encoding" with
  the "Bytes:" dump, once per input, byte replaced by U+FFFD.

`scan_characters` now distinguishes the two via the remaining-length rule.
Verified byte-identical vs oracle on 13 shapes incl. `<r>ab\xc3`,
`<r>ab\xe2\x82` (two per-byte errors), `<r>ab\xf0\x9f\x98` (incomplete F0
error followed by an I/O error on the now-dangling continuation 0x9F),
`<r>a\xc3b</r>`, `<r>ab\xc3\xc3`, `<r>\xc3`.

### 3.3 "Bytes:" dump must stop at the input end

error.c xmlFormatError breaks the 4-byte dump at `input->end` (no zero
padding). The capture now records `(bytes, len)` and the formatter prints
only the real bytes (`<r>ab\xc3\xc3` → "Bytes: 0xC3 0xC3", not
"... 0x00 0x00"). The payload type changed to `Option<([u8; 4], usize)>`
through tokenizer → state → errors.

### 3.4 "Sequence ']]>' not allowed" caret points at the RUN START

xmlParseCharDataInternal raises XML_ERR_MISPLACED_CDATA_END mid-scan while
`input->cur` still sits at the start of the character-data run (cur is only
committed at the run's callback flush), so the printed window/caret point at
the run start — not at the `]]>`. Recorded at `seg_start`. Verified for
every text length 0..=128 and for a second text run after an element
(`<a>x<b>yyyy]]>z</b></a>` → caret under the second run's first `y`).

### 3.5 NUL and control bytes

- Text NUL: xmlCurrentChar's c==0 branch first raises the composed
  "Invalid character: Char 0x0 out of allowed range\n\n" (xmlFatalErr:
  xmlErrString "Invalid character" + ": " + info, whose own trailing
  newline creates the blank line), then the PCDATA-invalid path adds
  "PCDATA invalid Char value 0". Verified for NUL at every position.
- Attribute values: a control char that is not an XML Char (`!IS_BYTE_CHAR`
  — NUL, 0x1-0x8, 0xB, 0xC, 0xE-0x1F) raises "invalid character in
  attribute value\n" (XML_ERR_INVALID_CHAR) and is replaced with U+FFFD in
  the value (xmlSBufAddReplChar). Verified incl. tab (legal, space-
  normalized) and 0x0B.

### 3.6 XML names must not contain '+'

The §16.6 ASCII-name fast path admitted '+' (`ascii_name_byte` and the
per-char fallback); upstream xmlParseName/xmlIsNameChar does NOT (names
stop at '+'): `<a+b/>` → "error parsing attribute name", `<a x+y="v"/>` →
"Specification mandates value for attribute x". Removed '+' from both name
scanners; verified `<a-b/>`/`<a.b/>`/`<a_b/>` still match (legal).

### 3.7 Self-closing '/' requires '>' and end tags require '>'

- `<a/foo>` was silently accepted as `<a/>` + text. Upstream
  xmlParseStartTag2 only stops its attribute loop at '/' when NXT(1)=='>';
  any other '/' falls into xmlParseAttribute2, whose name parse fails on
  '/': "error parsing attribute name" + "Couldn't find end of Start Tag".
  Verified `<a/foo>`, `<a /foo>`, `<a/ >`, `<a/b/>`, `<a b="c"/x>` vs the
  oracle; valid `<a/>`, `<a />`, `<a b="c"/>` unchanged.
- End tags: `</a+b>`, `</a x>`, `</a/>` were silently accepted. Upstream
  xmlParseEndTag2: SKIP_BLANKS then RAW != '>' → xmlFatalErr(
  XML_ERR_GT_REQUIRED) "expected '>'". Verified incl. legal whitespace
  (`</a >`, `</a\n>`, `</a\t>` — allowed by grammar) and EOF
  (`</a` → unterminated, unchanged).

## 4. Residual G2 stderr divergences (pre-existing, outside the scan
surface; each is its own upstream error path)

174 candidate-only lines across 29 files: the planted-`<` garbage-tag
cluster (nested start-tag error ordering "expected '>'", QName/namespace
diagnostics for malformed `xmlns`-ish attributes, PI "space expected" vs
"never end"), `t_mixed_controls` (I/O encoding error is raised ONCE per
input upstream via the XML_INPUT_ENCODING_ERROR flag — not yet modeled),
`e_doctype_trunc` (the known DTD declaration-truncation gap, errNo 118
family), unterminated-CDATA message variants ("CData section not
finished"/"Unregistered error message" vs "Premature end of data in CDATA
section"), and the xml:id-c14n/bogus.xml/test_broken.xml fixtures. Tracked
for the phase's remaining divergence work.

## 5. §16.7.5 AVX-512 policy note

No AVX-512 auto-selection yet: the quick wall-clock measurements taken at
module-commit time showed scalar 0.020 s / avx2 0.013 s / avx512 0.013 s on
a 27 MB one-long-text doc, with run-to-run load variance (±20 %) larger
than the avx2-vs-avx512 delta. The size/composition crossover measurement
(avx2 vs avx512, tiny→huge) moves with §16.8's measurement court (spec
§16.16: thresholds must be evidence-driven and holdout-validated, and §16.5
criterion work lands first).

## 6. Gates

- `cargo test --lib`: 1267 pass / 0 fail / 1 ignored.
- §16.7.7 court: G1 PASS (all backends, stdout+stderr); G2 trees
  byte-identical; see §2.
- Oracle A/B on every fix class: byte-identical + matching exit codes
  (details in §3).
- PHP six-gate (`cand-six-gate.sh`): 1250 passed / 0 failed.
- Parse fuzzer per backend (ASan): scalar/avx2/avx512 clean (see §2).
