# libxml-rs

**Phase 11: Historical matrix — semantic epochs for libxml2/libxslt behavior.**

Custodial native-Rust reimplementation of the **libxml2 + libxslt** ecosystem:
a forensic reconstruction of observable behavior, implemented in native Rust,
with C ABI compatibility for drop-in replacement.

This is **not** an XML crate. This is **not** an XSLT crate. This is **not** a wrapper.
This is a custodial forensic archive and native-Rust reimplementation of
the complete observable behavior of libxml2 and libxslt across their
historical lifetimes.

---

## Current Status: Phase 11 — Historical matrix (§85)

Phase 11 delivers the cross-version archaeology of **§41 (historical oracle matrix), §42
(version fingerprints), §51 (multi-version triangulation)**. Per §85:

> *Deliverable: The project can explain how current behavior came to exist.*

A configurable oracle matrix (`oracle/historical/`) builds 12 historical libxml2
releases (2.7.8 → 2.15.0) plus 5 libxslt releases (1.1.26 → 1.1.42) from the archaeology
git clones and runs a 28-case behavioral corpus against every oracle and the system
2.15.3/1.1.45 binaries, capturing byte-exact stdout/stderr/exit and per-version sha256
fingerprints:

- **`oracle/historical/build.sh`** — era-tolerant oracle builder (era tag spellings, autotools modernizations, `--without-threads` for modern glibc)
- **`oracle/historical/run_matrix.sh`** — matrix runner + per-case epoch grouping; `results/matrix.json` holds version→case→sha256
- **`atlas/SEMANTIC_EPOCHS.md`** — the epoch map: 10 behavioral epochs (E-001…E-008 + exit-code epochs) correlated with the exact upstream commits/NEWS that created them
- **`courts/suites/historical/HIST-EPOCH-*.json`** — §43 casefiles; **`courts/receipts/historical-matrix-2026-08-29.json`** — §44 receipt

Headline findings (all correlated to upstream commits in the atlas):

- `xmllint --xpath` node-set output gained newline separators in **2.9.10** (commit `da35eeae`, an upstream-documented breaking change)
- The 2.9.10 parser regression (`EndTag: '</' not found`) was fixed in **2.9.11** (`de5b624f`); the second parse-error diagnostic was dropped entirely in **2.12**
- A **2.13.0** cluster: parser/validation exit codes reworked (1→4, 4→3), entity-in-attribute errors reported twice, entity-decl debug nodes became `TEXT compact` (`8d04f0ee`)
- **2.15.0**: `--html` dumps became single-line (newline writes removed from `HTMLtree.c`); `--valid` with no DTD returns exit 0
- libxslt core transform output is a **stable epoch** — byte-identical from 1.1.26 (2009) to 1.1.45

### Phase 11.1 (historical)

Phase 11.1 (11.1-A … 11.1-Z) sealed the forensic surface census and parity
closure, and the **11.1-Z.1 evidence/packaging amendment** (0.1.0-alpha.34)
sealed the three-DSO ELF contract and the residual-ledger corrections.
**11.1-Z.2** (0.1.0-alpha.35) added the function-signature ABI plane: the
**ABI-FUNCTION-SIGNATURE** court mirrors every export across the oracle
header / candidate header / actual Rust `extern "C"` signature (3259
compared, 0 findings) and caught a whole defect class — the allocator hooks
(`xmlMemSetup`/`xmlGcMemSetup` missing `mallocAtomicFunc` and the `int`
return, two sources of allocator truth), shifted register layouts
(`xmlC14NExecute`, `xmlAutomataNewCountTrans`) and 20+ stale pre-2.10
signatures. The **ALLOCATOR-HOOK** differential court proves the merged
single-source-of-truth allocator byte-identical with the oracle. The
**CUSTODIAN-COMMENTARY-DRIFT** court pins every residual/court/epoch/
receipt reference in source commentary and bans embedded mutable counts.
The **DSO-STATE-COHERENCE** court pins a documented bounded divergence
(R-000177): the whole-archive libxslt/libexslt facades are the only
consumer-linkable construction, so they carry private copies of the core
state — hooks/globals installed through one DSO are not observed by the
others (the ELF contract itself — SONAMEs, NEEDED chains, export surfaces —
is verified by DSO-LOADER).
**11.1-Z.3** (0.1.0-alpha.36) closed the allocator-UB and proof-scope
accounts. The default allocator was invalid-layout UB (Rust `std::alloc`
with fabricated `Layout`s); it is now plain libc `malloc`/`realloc`/`free`/
`strdup` (upstream 2.15.0 `globals.c` defaults), untracked exactly like the
oracle, with the debug-named surface (`xmlMemMalloc`/`*Loc`) keeping the
registry — R-000178, proven by the new **ALLOCATOR-DEFAULT-001** differential
court (byte-identical incl. `xmlMemSize`/`xmlMemUsed`/`xmlMemBlocks`
exactness). The **ABI-FUNCTION-SIGNATURE** court was hardened to be
fail-closed and oracle-isolated: fully separated compile environments with
per-include-origin contamination checking, zero silent omissions (every
header and every declaration accounted), and two fingerprints —
SOURCE_PROTOTYPE (canonical base, pointer depth, pointee const/restrict,
typedef identity, FULL fn-pointer signatures) and MACHINE_ABI (class, width,
signedness, depth) — which found 25+ real candidate divergences that the
old lossy normalization could not see: signedness (`xmlBufferGrow`/
`xmlDictSetLimit`/`xmlOutputBufferGetSize`), wrong arities
(`xmlValidateAttributeDecl` 4→3 args), wrong returns (`xmlXPathCastBooleanToNumber`
int→double, `xmlCheckVersion` int→void, `xmlInitThreads` int→void), wrong
fn-pointer typedefs (`xsltPreComputeFunction`/`xsltTopLevelFunction`), an
invented `xmlTextReaderGetAttributeIndex` API (removed), `char`/`xmlChar`
pointee drifts, and the `xmlAttributeTable` typedef identities. The
**CUSTODIAN-COMMENTARY-DRIFT** census now GATES the verdict on zero
unaccounted safety sites via proof scopes: 1811/1811 unsafe functions and
5235/5235 unsafe blocks accounted (local proof 2514, enclosing proof scope
2720, classified-generated 1), plus the rustdoc gate
(`RUSTDOCFLAGS="-D warnings" cargo doc`), and the `xslHandleDebugger`
non-obligation is reclassified HEADER_DECLARED_ORACLE_DSO_ABSENT (the
oracle DSO itself does not export it).
**11.1-W (generated parity matrix)** replaced every hand-typed
headline count with evidence: `tools/evidence/generate_all.py` regenerates all
six canonical ledgers (`atlas/PARITY_MATRIX.json`, `atlas/SURFACE_RECONCILIATION.json`,
`atlas/API_PARITY_LEDGER.json`, `atlas/ABI_PARITY_LEDGER.json`,
`atlas/HISTORICAL_SURFACE_EPOCHS.json`, `atlas/PARITY_OBLIGATIONS.json`) and
their Markdown views, `--check` proves byte-reproducibility, and
`tools/evidence/readme_counts.py` generates the Project Status and
test-coverage tables in this README. The obligations ledger now covers all
three oracle projects (libxml2 2.15.3, libxslt 1.1.45, libexslt 0.8.25):
**0 missing** across 1683 obligations, with the residual closure loop at
**79 FIXED / 3 OPEN** (82 residuals; the OPEN entries are R-000157
UNRESOLVED — iconv/ICU-only encodings, a real executed-platform gap pending
an iconv/ICU backend — R-000168, the unexecuted-platform obligation, and
R-000177, the documented cross-DSO state partitioning of the facades, a
deliberately-open Phase-12 architectural target).
The 11.1-Z.1 amendment shipped three real ELF DSOs (core `libxml2.so.16` +
post-link facades `libxslt.so.1` and `libexslt.so.0` with the upstream NEEDED
chain), fixed the parity-matrix per-project DSO accounting, bound the
generated-evidence input identities (Doxygen inventories + headers), corrected
the verification-ladder state machine, and added the real
PREPROCESSOR-SURFACE / AST-SURFACE courts.

### Phase 10 (historical)

Phase 10 delivered the **`xmllint`** and **`xmlcatalog`** command-line tools
(§36):

Both tools are faithful native-Rust ports of the upstream programs, verified
byte-for-byte against the system libxml2 2.15.3 binaries:

- **`xmllint` CLI** — `--debug`, `--copy`, `--format`, `--valid`/`--postvalid`, `--dtdvalid`, `--xpath`/`--xpath0`, `--xinclude`, `--html`/`--xmlout`, `--noent`, `--no-compact`, `--encode`, `--recover`, `--dropdtd`, `--pedantic`, `--noout`, `--quiet`, `--nonet`, `--huge`, … with upstream exit codes (0/1/3/4) and the upstream `file:line: parser error : MSG` + source-line/caret diagnostics
- **`xmlcatalog` CLI** — `--create`, `--add`, `--del`, `--resolve` (system/public/URI), `--noout`, `--shell` (interactive `resolve`/`system`/`public`/`add`/`del`/`dump`/`help`/`exit`), SGML→XML catalog conversion, upstream exit codes
- **Compact text nodes** — the parser reproduces libxml2's `XML_PARSE_COMPACT` inline text storage (≤15 bytes in the node struct), so `--debug` dumps show `TEXT compact` exactly like the oracle; merged/entity-interrupted text is correctly non-compact
- **DTD validation diagnostics** — `--valid` output (messages, caret placement, exit codes) matches the oracle for both no-DTD and declaration errors
- **Entity expansion** — `--noent` re-parses declared-entity content through the input stack (nested references and markup entities included), matching upstream trees
- **HTML serialization** — meta-charset insertion, upstream formatting rules (p/pre/param never formatted, single-child and inline elements inline), HTML document headers
- **1183 passing tests**: `cargo test --lib` — 0 failures
- **Differential oracle parity**: a 44-case CLI suite (`target/difftest_summary.sh`) is **byte-identical** to the system tools (stdout + stderr + exit codes), plus a 30+ case edge corpus (entities, DTDs, HTML, compact/no-compact, debug dumps, XPath)

### Phase 9 (historical)

Phase 9 delivered the **complete EXSLT module set** (§35) and the
**`xsltproc` command-line tool** (§36):

All seven EXSLT modules are implemented natively and registered through the
process-wide EXSLT registry (`exsltRegisterAll`, mirroring upstream):

- **`exsl:` Common** — `exsl:node-set` (with real result-tree-fragment support), `exsl:object-type`
- **`math:`** — `math:max`, `math:min`, `math:sin`, `math:cos`, `math:tan`, `math:constant`, `math:power`, `math:sqrt`, `math:log`, `math:random`, …
- **`set:`** — `set:difference`, `set:intersection`, `set:distinct`, `set:has-same-node`, `set:leading`, `set:trailing`
- **`str:`** — `str:tokenize`, `str:padding`, `str:concat`, `str:split`, `str:replace`, `str:align`, `str:encode-uri`, `str:decode-uri`
- **`dyn:`** — `dyn:evaluate`, `dyn:element`, `dyn:attribute`, `dyn:map`, `dyn:call`, …
- **`date:`** — `date:date-time`, `date:date`, `date:time`, `date:year`, `date:month-in-year`, `date:day-in-month`, `date:day-of-week-in-month`, `date:format-date`, `date:add`, `date:difference`, `date:seconds`, `date:day-name`, …
- **`func:`** — `func:function`, `func:result` (extension-function declarations)
- **`exsltRegisterAll` C ABI export** — mirrors upstream; `xsltproc` calls it at startup
- **`xsltproc` CLI** — full option surface (`--param`, `--stringparam`, `--output`, `--noout`, `--html`, `--encoding`, `--xinclude`, `--profile`, `--maxdepth`, `--maxvars`, `--nonet`, `--nowrite`, …) with upstream exit codes (1–11)
- **RTF support** — variables with inline content become context-owned result tree fragments; `exsl:node-set($var)/path` navigation works
- **1183 passing tests**: `cargo test --lib` — 0 failures
- **Differential oracle parity**: a 12-case `xsltproc` corpus (basic transform, `count()`/AVTs, `exsl:node-set`/`math:`/`set:`/`str:`, predicates, attribute string-values, `xsl:if`/`xsl:when`, numbering, descending `xsl:sort`, `key()`, `call-template` with params, `method="html"`) is **byte-identical** to the system libxslt 1.1.45 `xsltproc` (stdout + exit codes)

### Underlying subsystem fixes landed during Phase 9

| Fix | Surface | Detail |
|-----|---------|--------|
| XPath core functions in XSLT | xslt | The transform context now registers the XPath 1.0 core function library — before, every XPath function call (`count()`, `substring()`, …) failed as unknown |
| AVT evaluation | xslt | `{expr}` attribute value templates evaluated in literal attributes and `xsl:element`/`xsl:attribute`/`xsl:processing-instruction` names (XSLT 1.0 §7.6.2) |
| RTF variable ownership | xslt | Inline variable content is deep-copied into a context-owned RVT (freed exactly once at context teardown) — fixes a double-free and enables `exsl:node-set` |
| Node string-value | tree | `node_get_content` concatenates all descendant text (XPath 1.0 string-value), not just direct text children |
| Caller parameter format | xslt | `xsltApplyStylesheet` params parsed as upstream `(name, value)` pairs with `{uri}name` namespace form |
| `date:` no-arg default | exslt | `date:date()`/`date:time()` and the component functions default to the current date-time (EXSLT spec) |

### Phase 8 (historical)

Phase 8 implemented the complete native-Rust **XSLT 1.0 engine**: stylesheet
compilation, template matching, pattern compilation, variable/parameter binding,
keys, sorting, numbering, imports/includes, extensions, security, serialization,
and the transform runtime (§31–§34). The XSLT engine operates exclusively on
the Rust libxml implementation — no upstream libxml2/libxslt is loaded, linked,
or shelled out to (§31):

```text
Rust CLI → Rust libxslt compatibility layer → Rust XSLT engine
        → Rust XPath implementation → Rust libxml tree/parser/serializer
```

Subsystem fixes landed during Phase 8:

| Fix | Surface | Detail |
|-----|---------|--------|
| SAX2 namespace resolution | parser | `startElementNs` receives split prefix/localname + resolved URIs; element and attribute namespaces now attached by the tree builder |
| Absolute path evaluation | XPath | `/root/item` evaluates from the document node, not the root element (XPath 1.0 `/` semantics) |
| XPath C ABI helpers | ABI | `xmlXPathObjectCopy`, `xmlXPathCastToString`, `xmlXPathCastStringToNumber`, `xmlXPathCmpNodes`, `xmlXPathNodeSetCreate` |
| Node content getter | tree | `node_get_content` (upstream `xmlNodeGetContent` semantics) |
| Template-content ownership | xslt | Template content belongs to the stylesheet document; `xsltFreeTemplate` no longer double-frees it |
| `node()`/`text()` patterns | xslt | Bare node-test calls translate to steps with correct §5.5 priorities |
| `match="/"` semantics | xslt | Matches only the document node, not the root element |

All six Phase 8 residuals are documented in [`atlas/RESIDUAL_LEDGER.md`](atlas/RESIDUAL_LEDGER.md) (R-000101–R-000106).

### Build

```sh
cargo build                          # Build library + CLI binaries
cargo build --lib                    # Build only the library
cargo test --lib                     # Run library tests (1183 passing)

# Test C consumer compilation against our headers:
gcc -I include courts/suites/sanity/ABI-STRUCT-NODE-0001-abicheck.c -o /tmp/abicheck
clang -I include courts/suites/sanity/ABI-ENUM-0001-enumcheck.c -o /tmp/enumcheck

# Build and run oracle container:
docker build -f docker/Dockerfile.oracle -t libxml-rs/oracle:2.12.0 docker/
```

### Published artifacts

- crates.io: [`libxml-rs`](https://crates.io/crates/libxml-rs) `0.1.0-alpha.36`
- GitHub: <https://github.com/infinityabundance/libxml-rs>

### Oracle verification

The oracle Docker container builds libxml2 2.12.0 and libxslt 1.1.39 from source.
The candidate (`libxml-rs`) does not link against system libxml2/libxslt — verified
by the oracle contamination court.

---

## Test coverage by subsystem

<!-- GENERATED-TESTCOVERAGE:START -->
| Subsystem | Tests |
|-----------|------:|
| XPath 1.0 | 128 |
| URI | 69 |
| Encoding | 65 |
| XML Schema (XSD) | 62 |
| DTD validation | 56 |
| RELAX NG | 56 |
| XML parser + SAX | 53 |
| XSLT patterns | 46 |
| I/O | 44 |
| Regex | 44 |
| Schematron | 40 |
| C14N | 39 |
| DTD | 35 |
| XML Reader | 35 |
| Entities | 31 |
| HTML | 31 |
| Tree/ownership | 29 |
| XSLT transform | 26 |
| XInclude | 23 |
| Catalog | 22 |
| XML Writer | 20 |
| Automata | 16 |
| XPointer | 15 |
| XSLT numbering | 14 |
| ABI allocator | 13 |
| Debug | 11 |
| Char validation | 9 |
| Compatibility profiles | 9 |
| EXSLT dates | 9 |
| Hash | 9 |
| ABI (xslt exports) | 8 |
| List | 8 |
| Dictionary | 7 |
| EXSLT saxon | 7 |
| Globals | 7 |
| EXSLT strings | 6 |
| String | 6 |
| EXSLT math | 5 |
| Errors | 5 |
| XSLT compiler | 5 |
| XSLT params | 5 |
| XSLT security | 5 |
| EXSLT common | 4 |
| EXSLT sets | 4 |
| Serialization | 4 |
| XSLT serialization | 4 |
| XSLT stylesheet | 4 |
| XSLT variables/params | 4 |
| EXSLT registry | 3 |
| XSLT sorting | 3 |
| ABI data globals | 2 |
| EXSLT dynamic | 2 |
| Memory | 2 |
| Threads | 2 |
| XSLT extensions | 2 |
| XSLT keys | 2 |
| XSLT namespace alias | 2 |
| XSLT whitespace | 2 |
| ABI (xml2 exports) | 1 |
| EXSLT functions | 1 |
| XSLT documents | 1 |
| XSLT imports | 1 |
| XSLT misc (attrs) | 1 |
| **Total (1183 passing, 0 failed)** | |
<!-- GENERATED-TESTCOVERAGE:END -->

---

## Project Structure

```
libxml-rs/
├── Cargo.toml              # Single Cargo package (no workspace)
├── src/
│   ├── lib.rs              # Library entry point
│   ├── abi/                # C ABI compatibility layer (§4, §14)
│   ├── xml/                # libxml2 implementation (§1, §3, §31)
│   ├── xslt/               # libxslt implementation (§31–§34)
│   ├── exslt/              # EXSLT modules (§35)
│   ├── compatibility/      # Historical profiles, quirks, platform (§68, §69)
│   └── bin/                # CLI tools: xmllint, xmlcatalog, xsltproc (§36)
├── include/                # Compatible C headers (§15)
├── atlas/                  # Forensic archive (§7–§12)
│   ├── releases/           # Release manifests per version
│   ├── api/                # Public API inventories
│   ├── abi/                # ABI snapshots
│   ├── symbols/            # Symbol table comparisons
│   ├── config/             # Build configuration profiles
│   ├── standards/          # Standards mapping
│   ├── HISTORY.md          # Complete release history
│   ├── LORE.md             # Undocumented behavior archive
│   ├── QUIRKS.md           # Confirmed compatibility quirks
│   ├── PARITY_MATRIX.md    # Current parity status
│   ├── RESIDUAL_LEDGER.md  # Unexplained differences
│   └── SECURITY_HISTORY.md # Vulnerability custody
├── oracle/                 # Reproducible upstream build environment (§39)
├── courts/                 # Differential testing framework (§40–§50)
│   ├── schema.json         # Casefile schema
│   ├── suites/             # Court case suites
│   ├── receipts/           # Execution receipts
│   └── tools/              # Court runner
├── tools/                  # Archaeology and analysis tooling
│   ├── archaeology/        # manifest.py, apiatlas.py, symbols.py, delta.py, profileconfig.py
│   └── courts/             # Court runner
├── docker/                 # Reproducible Docker oracle images
├── docs/                   # Technical documentation
└── archaeology/            # Upstream git clones (immutable, offline)
```

---

## Build

```sh
cargo build              # Build the library and CLI binaries
cargo build --lib        # Build only the library
cargo test --lib         # Run library tests (1183 passing)
cargo build --release    # Optimized build (LTO, panic=abort)
```

The crate builds as `cdylib` + `staticlib` + `rlib`; the build script also
emits `libxml-2.0.pc`/`libxslt.pc` pkg-config files, `xml2-config`/`xslt-config`
scripts, and SONAME symlinks into the target directory.

---

## License

Licensed under either of:
- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

---

## Project Status

<!-- GENERATED-STATUS:START -->
| Dimension | Status |
|---|---|
| API completeness | libxml2 1395 oracle functions, 1395 fully reconciled; libxslt 232/232 reconciled; libexslt 13 oracle functions (evidence: atlas/PARITY_MATRIX.json, atlas/API_PARITY_LEDGER.json) |
| ABI compatibility | 0 mismatches across 937 measured entities (struct/enum layouts), verdict PASS (evidence: atlas/ABI_PARITY_LEDGER.json) |
| Parity obligations | 1683 obligations; 0 missing, 292 parity-verified by per-symbol courts (evidence: atlas/PARITY_OBLIGATIONS.json) |
| Subsystem census | 85 subsystems classified; verdicts: IMPLEMENTED_UNVERIFIED 73, PARTIAL 4, UNOBLIGATED 8 (evidence: atlas/SUBSYSTEM_CENSUS.json) |
| Surface reconciliation | libxml2: doxygen 1374 / AST 1403 / DSO 1395 functions; libxslt: 235 / 231 / 232 (evidence: atlas/SURFACE_RECONCILIATION.json) |
| Historical surface epochs | libxml2 2785 entities across 11 boundaries (evidence: atlas/HISTORICAL_SURFACE_EPOCHS.json) |
| Test coverage | 1183 passing, 0 failed, 1 ignored (`cargo test --lib`, evidence: atlas/TEST_COUNTS.json) |
| C headers | gcc & clang header-compile courts green (596/596, evidence: courts/receipts/header-compile-*) |
| CLI parity | `xmllint` + `xmlcatalog` + `xsltproc` differential oracle parity (evidence: courts/receipts/CLI-*) |
| Oracle infrastructure | 12 historical libxml2 + 5 libxslt oracles + system 2.15.3/1.1.45/0.8.25 oracles; evidence: oracle/historical, atlas/DOXYGEN_SURFACE_ATLAS.json |
| Downstream testing | Not started (Phase 12) |
<!-- GENERATED-STATUS:END -->

See [`atlas/PARITY_MATRIX.md`](atlas/PARITY_MATRIX.md) for the detailed,
evidence-bounded parity matrix and [`atlas/RESIDUAL_LEDGER.md`](atlas/RESIDUAL_LEDGER.md)
for the residual ledger.
