# Phase 14.26 — ZTS gate + R-000177 cross-DSO loader bridge: six-extension suite GREEN under ZTS (0 failures)

Gates: candidate ZTS (`phpbuild-z`, `/srcz/php-src`, `--enable-zts` config) full
six-extension file-list run = **1290 tests / 1250 passed / 40 skipped / 0 failed**
(log `/out/zts-cand-six-excl.log`), identical numbers to the NTS seal
(`phpbuild-c` rerun on the changed DSOs = 0 failed, `/out/nts-cand-six-excl-1426.log`).
cargo fmt clean, `cargo test --lib` 1242 pass. Valgrind 0 errors on the
repaired xinclude path under ZTS php.

## The ZTS-only failure and its root cause

The ZTS gate had exactly one failure: `ext/xsl/tests/xinclude/xinclude.phpt`
(`doXInclude`). NTS candidate and oracle both passed; ZTS candidate emitted
`<container/>` (the include never happened).

Root cause chain (instrumented probes, `zts-xp2-abs-probe.php`):

1. The candidate ships the upstream-faithful three-DSO layout: the real core
   `libxml2.so.16` (the cdylib) plus whole-archive facade DSOs
   `libxslt.so.1`/`libexslt.so.0` that re-link the ENTIRE staticlib
   (R-000177, pinned by DSO-STATE-COHERENCE). The facades therefore carry
   private copies of the crate AND of its `thread_local!` cells.
2. php ext/libxml MINIT registers its VCWD-aware streams loader through
   `xmlParserInputBufferCreateFilenameDefault(php_libxml_input_buffer_create_filename)`
   — a symbol the php binary binds to the CORE DSO (the facade hides the
   xml* surface). The hook lands in the CORE's per-thread cell.
3. `document('data.xml')` inside an XSL transform runs in the FACADE (php
   ext/xsl links libxslt). The facade's internal loads consult ITS private
   TLS cell — empty. Under NTS this is masked because php's chdir is real
   and relative raw opens succeed. Under ZTS, php virtualizes chdir
   per-thread while `/proc/self/cwd` stays at the process start dir, so the
   raw relative open of `data.xml`/`xincluded.xml` fails (errno=2) — and the
   loader that would resolve through php's virtual cwd is invisible across
   the partition. The ZTS oracle passes because upstream ships ONE core DSO
   (real libxslt NEEDs real libxml2): every internal open observes the hook.

Evidence from the instrumented run: MINIT `func=Some` then (MSHUTDOWN)
`func=0x0` on ThreadId(1); in the xinclude engine `hook-set=false` and
`libc path fd=-1` for `xincluded.xml` even though the same process opened
`data2.xml` (relative) successfully through the loader — the classic
same-process, cross-DSO split.

## The fix: cross-DSO loader-slot bridge (R-000177 mitigation)

New accessors in `src/xml/globals/mod.rs`:

- `get_parser_input_buffer_create_filename_value_cross_dso()` /
  `get_output_buffer_create_filename_value_cross_dso()`: read the local
  per-thread cell first; when it is empty, consult the process-visible
  exported value accessor (`__xmlParserInputBufferCreateFilenameValue` /
  `__xmlOutputBufferCreateFilenameValue`, both in the core's
  `libxml2.syms` export map; the facades hide them) via
  `dlsym(RTLD_DEFAULT)` (once-cached) and read the CURRENT thread's cell in
  the exporting (core) DSO. This restores upstream's single-core-DSO
  property: a loader php registers through the core's exported setter is
  observed by every internal open, whichever DSO copy runs it.

Single-DSO links (cargo tests, published-crate fallback, non-Linux) are
unchanged: dlsym resolves the same DSO's own export, i.e. the very same
cell, so the HOSTILE-THREADS per-thread invariant (a handler installed on
one thread is not observable from another) is preserved — the foreign read
is always same-thread, cross-DSO.

Consumption sites switched to the bridge:
- `src/abi/exports_parser.rs`: `call_loader_materialize`,
  `open_filename_routed`, the `default_external_entity_loader` gate.
- `src/abi/exports_html.rs`: `htmlCreateFileParserCtxt`.
- `src/xml/io/mod.rs`: `output_buffer_create_filename_routed`.
- `src/xml/xinclude/mod.rs` `io_read_file` (the gate's failing path).

## Verification

- Single phpt under ZTS: `PASS doXInclude`.
- ext/xsl under ZTS: 83 tests / 78 passed / 5 skipped / 0 failed.
- Full six-extension ZTS file-list gate (oracle-parity exclusion applied,
  same methodology as the NTS seal): 1290 / 1250 / 40 skipped / **0 failed**.
- Full NTS rerun on the changed DSOs: 1290 / 1250 / 40 skipped / **0 failed**.
- `cargo test --lib`: 1242 passed / 0 failed.
- Valgrind (ZTS php, xinclude probe): 0 errors.
- Probes `zts-xp2-abs-probe.php` compare the ZTS candidate byte-for-byte
  with the NTS oracle on (a) doXInclude with absolute href, (b) plain
  relative `document()`, (c) DOM `LIBXML_XINCLUDE` — identical output.

## Notes for the record

- The remaining 1-in-1291 failure WITHOUT the exclusion is the known broken
  `xmlwriter_toStream_encoding_shiftjis.phpt` (its `.exp` demands an empty
  comment `<!---->` which no correct libxml2 emits — the oracle writes real
  Shift_JIS bytes 0x82 0x9F). Under ZTS the oracle tree's copy of that phpt
  had been rewritten by earlier harness runs to carry the true SJIS bytes in
  `--EXPECT`, so the ZTS oracle "passes" it trivially; the candidate tree
  holds the pristine phpt. Parity holds: candidate and oracle emit identical
  bytes; the exclusion stays (Phase-14.27 closes R-000198, the underlying
  writer-encoder gap, tracked in the ledger).
- Phase 14.27: implement the writer output-encoder install
  (R-000198, upstream `xmlTextWriterStartDocument`/`xmlTextWriterSetOutput
  Encoding` set `out->encoder` + conv) and the encoding_rs-backed Shift_JIS/
  EUC-JP handlers (R-000157 slice).
