# Phase 14 — PHP court tracked state (auto-updated)

Refresh 14.26 (2026-09-04): the earlier 1098-line Phase-14.3 running log is
preserved in git history; this file now tracks the sealed end-state and the
receipt trail.

## Sealed end-state: six-extension PHP gate at ZERO failures (NTS and ZTS)

- Oracle (phporacle-c, upstream libxml2 2.15.3 + libxslt 1.1.45 at
  /usr/local/lib, Iconv+ICU): 1291 tests / 1251 passed / 40 skipped / 0 failed
  (unmodified suite).
- Candidate (phpbuild-c, host target/debug DSOs at /candidate):
  started 321 failed (php-14-3-baseline) → **0 failed**.
- ZTS seal (phpbuild-z, candidate php built with --enable-zts): the same
  six-extension file-list gate = **0 failed** (1290 / 1250 passed / 40
  skipped), achieved via the Phase-14.26 R-000177 cross-DSO loader-slot
  bridge (php ZTS virtualizes chdir per-thread; the whole-archive libxslt
  facade's private TLS could not see the streams loader php registers
  through the core DSO's exported setter — xsl document()/xinclude failed;
  see php-14-26-zts-green-20260905/).
- Green-gate configuration (both sides identical): the full six-extension spec
  (`ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl`)
  expanded to an explicit .phpt file list minus the documented oracle-parity
  exclusion `php-court-oracle-parity.exclude`
  (`xmlwriter_toStream_encoding_shiftjis` — its checked-in .exp demands an
  empty comment, unsatisfiable by any correct libxml2; the oracle fails it
  identically). Result: **1290 tests / 1250 passed / 40 skipped / 0 failed**,
  reproducible across repeated file-list and directory-mode gates
  (`/out/xpe-six4*` logs).
- `cargo test --lib` 1242 pass; valgrind 0 errors on the new paths
  (fromStream streaming + depth-2500 xml_parse_into_struct).
- Published: 0.1.0-alpha.39.

## Failure-count arc (six-extension gate logs; receipts under
  courts/receipts/phase-14/php-14-NN-*/)

- 14.3 baseline (3f42436b): 321 failed (31 crash-class)
- 14.3.x … 14.16: family-by-family root-cause closures (dom classic +
  modern/*, simplexml, xml expat-compat, xmlreader, xmlwriter, xsl) →
  26 failed (receipts php-14-4 … php-14-16)
- 14.17 (7650b116): xmlreader cursor/schema/relaxng/error cluster + XSD ns
  engine: 26 → 20
- 14.18 (dfd1a337): DTD entity orig + dom bug67081: 20 → 18
- 14.19 (5dbaba97): xsl params/template-guards/EXTRA write cluster: 18 → 13
- 14.20 (3c8de59b): dom big-lines psvi + windows-1252 override: 13 → 10
- 14.21 (71fbe227): dom family to zero: 10 → 4
- 14.22 (9327331f): xsl copy/element ns fixup + pattern priorities + URI
  matching: 4 → 3
- 14.23 (c27bf9b6): xsl xinclude document() + double-free: 3 → 2
- 14.24 (302721fe): xmlreader fromStream_broken_stream (partial-doc streaming
  + deferred EOF finalize): 2 → 1 — last candidate-driven failure closed
- 14.25 (5a4d68d8): parser deep-document stack-safety (bug65236; parse_element
  split) + oracle-parity exclusion wired into the gate: 1 → **0**
- 14.26 (ZTS seal): R-000177 cross-DSO loader bridge → the six-extension
  gate is 0 failed under **both** NTS and ZTS
  (receipt php-14-26-zts-green-20260905)
- 14.27 (writer encoder + native SJIS/EUC-JP): R-000198 FIXED —
  xmlTextWriterStartDocument installs the declared encoding's handler on
  out->encoder + conv (upstream xmlwriter.c); new encoding_rs-backed
  Shift_JIS/EUC-JP handlers close the R-000157 slice; the byte-parity probe
  is byte-identical to the oracle incl. the unmappable → &#NNNN; path
  (receipt php-14-27-writer-encoder-sjis-20260905).
- 14.28 (iterative element driver): R-000199 FIXED — the recursive
  parse_element → parse_element_content → parse_element descent became a
  flat token loop over an explicit heap stack of open-element frames; ext/xml
  xml_parse now parses depth-100000 crash-free where depth-4000 SEGFAULTed
  (oracle: 20000); SAX event sequences + DOM serialize + the 2048 tree cap
  are oracle-identical (receipt php-14-28-iterative-parser-20260905).
- 14.29 (encoding backend completion): R-000157 FIXED — the full enumerated
  iconv/ICU set is native: encoding_rs-backed ISO-8859-2..11/13..16 +
  ISO-2022-JP, native UCS-2 (host-order LE) + UCS-4LE/BE codecs and a
  glibc-derived EBCDIC 037 table; the parser input layer now
  whole-buffer-decodes any registry-served declared encoding and
  pattern-detects the UCS-4/EBCDIC/BOM-less-UTF-16 family (upstream
  xmlDetectCharEncoding); output + input probes are cmp-identical to the
  oracle (receipt php-14-29-encoding-backend-20260905).

## Register of residuals opened/closed by the court

- R-000198 (FIXED, Phase 14.27): writer output-encoder install — see the
  14.27 receipt; the pristine php phpt stays excluded (its .exp demands an
  empty comment, unsatisfiable by any correct libxml2).
- R-000199 (FIXED, Phase 14.28): iterative element driver — see the 14.28
  receipt.
- R-000157 (FIXED, Phase 14.29): the enumerated iconv/ICU encoding set is
  native (handlers + parser input side) — see the 14.29 receipt; bounded
  remainder: iconv names beyond the enumeration and multi-flush ISO-2022-JP
  escape state.
- R-000177 (OPEN): cross-DSO state partitioning — Phase 14.26 added the
  same-thread cross-DSO loader-slot bridge (dlsym of the core's exported
  `__xml{Parser,Output}BufferCreateFilenameValue` accessor) restoring
  upstream single-core hook visibility under php ZTS; the structural
  partition remains pinned by DSO-STATE-COHERENCE.
- Open global residuals (atlas/RESIDUAL_LEDGER.json): R-000168, R-000177,
  R-000179 (3 open).

## Next

R-000177 cross-DSO observable-state coherence (the 14.26 bridge extended to
the remaining per-thread globals) and R-000179 (versioned-distro symbol
graph) are the open engine items; the bounded R-000157 remainder (iconv
names beyond the enumerated set; multi-flush ISO-2022-JP state) can follow;
Debian reverse-dependency court (Phase-14 consumer 4/4) is the next custodian
gate. See PROJECT_STATE.md "Phase 14" + "Immediate Next Actions".
