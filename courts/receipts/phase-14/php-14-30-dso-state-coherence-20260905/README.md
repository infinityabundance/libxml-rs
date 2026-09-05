# Phase 14.30 — R-000177 FIXED (cross-DSO state coherence) + R-000179 FIXED (versioned-distro profile) + R-000168 compile surface restored

## Gates

- **DSO-STATE-COHERENCE court: PASS — full parity** (all 10 observations match
  the oracle; previously the court PINNED the documented state partition).
- **PHP six-extension gate, NTS + ZTS: 1290 / 1250 passed / 40 skipped /
  0 failed each** (`xpe-six46.log` NTS had 1 failure, `xpe-six47.log` 0;
  `zts-cand-six-1430.log` 0). The single intermediate failure
  (`XMLDocument_createFromFile` php://memory) was a candidate bug found and
  fixed by the gate itself.
- `cargo test --lib` **1254 pass / 0 failed**; ABI differential courts
  byte-identical (globals-threading, allocator-default, callback-family,
  data-globals, save-family); valgrind 3.19 clean in the phpbuild container
  (dso probe + php://memory createFromFile/simplexml paths).
- **ELF-VERSIONING court 20/20 PASS** (14 original + 6 new versioned-profile
  cases).
- Cross-compile contract restored: `cargo check --lib --target
  aarch64-unknown-linux-gnu / i686-unknown-linux-gnu /
  armv7-unknown-linux-gnueabihf / x86_64-unknown-linux-musl` all 0 errors.

## What landed

### 1. R-000177 FIXED — three-DSO state bridges (full upstream parity)

The whole-archive libxslt/libexslt facades now observe consumer state installed
through the core DSO exactly like upstream's single shared libxml2 instance:

- allocator slots (dlsym'd `__xml*` accessor over the core's xmlMemSetup
  variables);
- node register/deregister hooks (bridged per-thread
  `__xmlRegisterNodeDefaultValue` / `__xmlDeregisterNodeDefaultValue` cells);
- external entity loader (core registration preferred) **and main-document
  resource loads** route through the registered external entity loader
  (upstream 2.14+ `xmlLoadResource`/`xmlCtxtNewInputFromUrl` layering,
  verified entry-by-entry against the executed oracle; the xmlTextReader does
  NOT consult the entity loader); custom-loader NULL fails silently; an empty
  loader result is a valid zero-length input (php://memory "Document is
  empty");
- fresh parser contexts snapshot the deprecated per-thread defaults
  (keepBlanks/replaceEntities) like `xmlInitParserCtxt`; keepBlanks is only
  ever lowered (NOBLANKS) — `xmlKeepBlanksDefault(0)` governs fresh-context
  reads exactly like the executed 2.15.3;
- the deprecated `xmlXxxDefault`/`xmlThrDef*` setters store unconditionally
  (0 included) and return the PREVIOUS value (the old conditional pattern
  made `xmlKeepBlanksDefault(0)` a no-op); LineNumbers twins return 1;
- the xslt default document loader passes the transform's parserOptions
  (XSLT_PARSE_OPTIONS = NOENT|DTDLOAD|DTDATTR|NOCDATA) — document() now loads
  external parsed entities like upstream;
- `parse_entity_content` loads external general parsed entities under
  validate/replaceEntities (non-NOENT) like upstream `xmlNewEntityInputStream`.

### 2. R-000179 FIXED — versioned-distro profile (libxml2.so.2)

A SEPARATE artifact `target/debug/versioned/libxml2.so.2.13.9` (SONAME
`libxml2.so.2`) carries the exact upstream `LIBXML2_2.x` named-version graph
derived from the authoritative distro DSO's
(`/usr/lib/libxml2.so.2.13.9`) `.gnu.version` tables
(`tools/packaging/libxml2-versioned.syms`, 43 nodes + the LIBXML2_2.15.0
terminal for 2.15-era exports). A distro-versioned consumer (linked against
the .2 SONAME, DT_VERNEED records LIBXML2_2.x requirements) runs against the
profile byte-identical (modulo the version string) with **zero** ld.so "no
version information available" warnings. The unversioned .16 core (executed
oracle parity) is untouched. Generated automatically by
`tools/packaging/versioned-profile.sh` after every link (linker-wrapper).

### 3. R-000168 — compile-expected surface restored

The 11.1-Z.2 x86_64 varargs shims had broken the non-x86_64 compile targets:
the four legacy `xmlParser{Error,Warning,Validity*}V` receivers and the
`XML_PARSER_*_SAX1` consts are now x86_64-gated with the SysV shim (other ABIs
get raw-message forwarding defaults), and `xsltCalibrateAdjust` widens c_long
to the i64 accumulator. aarch64 / i686 / armv7 / musl all compile 0-error
again. R-000168 remains OPEN by design (runtime execution outside Linux
x86-64 = the CI-matrix obligation).

## Evidence

- courts/receipts/phase-11/dso-state-coherence-20260905T004955Z.json
  (+ 005616, 011045 — all PASS full-parity)
- courts/receipts/phase-11/{globals-threading,allocator-default,
  callback-family,data-globals,save-family}-20260905T0057*.json
- courts/receipts/phase-12/elf-versioning-20260905T010556Z.json (20/20)
  (+ 010410, 011045)
- tools/abi/dso_state_coherence_probe.py (mode: full-parity)
- tools/packaging/{versioned-profile.sh, versioned-profile-gen.py,
  libxml2-versioned.syms}
- courts/suites/phase12/elf-versioning/{court-runner.sh,
  distro-versioned-consumer.c}
- atlas/RESIDUAL_LEDGER.json/.md — R-000177 FIXED, R-000179 FIXED, 1 open
