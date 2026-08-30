# Security History Atlas

Per §73: custody entries for historical vulnerabilities. Each entry records
affected versions, surface, trigger, observable symptoms, root-cause class,
fix version, fix semantic change, whether downstream-visible behavior changed,
and the modern regression court.

All commits below are resolved via `SRC-LIBXML2-GIT` / `SRC-LIBXSLT-GIT`
(`git show <hash>`). Confidence: `known` (verified from commit history);
`inferred` where noted.

**Policy (§5, §73):** do not publish weaponized exploitation. The objective is
compatibility custody and regression prevention. Where parity with a
vulnerability would require reproducing memory corruption, the divergence is
recorded as `SAFETY_DIVERGENCE` (see `docs/SECURITY.md`).

---

## 1. The parser-limits epoch (2008–2012)

### SEC-0001 — Default parser limits introduced (behavioral break)
- **Affected versions:** behavior change in 2.9.0 (commit `52d8ade7`,
  2012-07-30, "Introduce some default parser limits"); constant added to
  `parserInternals.h`; new error in `xmlerror.h`.
- **Surface:** parser (name length, dictionary size, lookup count).
- **Trigger:** very long names / large dictionaries / deep lookup.
- **Observable symptom:** new `XML_ERR_...` errors where none occurred before;
  documents that previously parsed now fail unless `XML_PARSE_HUGE` is set.
- **Root-cause class:** denial-of-service hardening.
- **Fix version:** 2.9.0.
- **Fix semantic change:** yes — **default behavior changed**. This is one of
  the most famous downstream-affecting changes in libxml2 history.
- **Downstream-visible behavior change:** yes (see `atlas/QUIRKS.md` QUIRK-0001).
- **Regression court:** `PARSER-LIMIT-*` courts (Phase 3).

### SEC-0002 — XML_MAX_TEXT_LENGTH (2009)
- **Affected versions:** added in commit `1fb2e0df` (2009-01-18),
  "add a new define XML_MAX_TEXT_LENGHT limiting the maximum size of a single
  text node" (note the upstream spelling `LENGHT`).
- **Surface:** parser text-node size limit.
- **Regression court:** `PARSER-TEXT-LIMIT-*`.

### SEC-0003 — XML_PARSE_HUGE (2008)
- **Affected versions:** commit `8915c150` (2008-08-26), "strengthen some of
  the internal parser limits, add an XML_PARSE_HUGE option". Present as
  `1<<19` in 2.8.0.
- **Surface:** parser option that lifts all hardcoded limits.

---

## 2. The 2013–2016 CVE wave

### SEC-0004 — CVE-2013-2877 (entity loop)
- **Fix commit:** `CVE-2013-2877` tag exists in git.
- **Surface:** parser entities.
- **Status:** details pending (see `_cve_fix_commits.json`).

### SEC-0005 — CVE-2014-0191 (doctype entity)
- **Fix:** `dd8367da` (2014-06-11) notes regression fixes from the original
  CVE-2014-0191 patch. Tag `CVE-2014-0191` exists.
- **Surface:** parser, doctype entity handling.

### SEC-0006 — CVE-2014-3660 (billion laughs)
- **Fix:** `be2a7eda` (2014-10-16) "Fix for CVE-2014-3660".
- **Regression fix:** `72a46a51` (2014-10-23) "Fix missing entities after
  CVE-2014-3660 fix" — **note the follow-up regression**: the CVE fix broke
  entity behavior and was itself patched. This is a classic §51
  multi-version-triangulation case: the CVE fix *changed observable entity
  semantics*.
- **Surface:** entity expansion.
- **Root-cause class:** exponential entity expansion (billion laughs).
- **Downstream-visible behavior change:** yes — entity expansion became
  bounded by default.
- **Regression court:** `PARSER-ENTITY-*`.

### SEC-0007 — CVE-2015-1819 (reader memory)
- **Fix:** `213f1fe0` (2015-04-14) "Enforce the reader to run in constant memory".
- **Surface:** reader API.
- **Downstream-visible behavior change:** reader constant-memory semantics.

### SEC-0008 — CVE-2015-5312 / CVE-2015-7497 / CVE-2015-7500 / CVE-2015-8035 / CVE-2015-8242 (2015 batch)
- **Fixes (all 2015-11-20):**
  - `69030714` CVE-2015-5312 "Another entity expansion issue"
  - `6360a31a` CVE-2015-7497 "Avoid an heap buffer overflow in xmlDictComputeFastQKey"
  - `f1063fdb` CVE-2015-7500 "Fix memory access error due to incorrect entities boundaries"
  - `f0709e3c` CVE-2015-8035 "Fix XZ compression support loop"
  - `8fb4a770` CVE-2015-8242 "Buffer overead with HTML parser in push mode"
- **Surface:** entities, dict, HTML parser push mode.
- **Regression courts:** `PARSER-ENTITY-*`, `DICT-*`, `HTML-*`.

### SEC-0009 — CVE-2016-xxxx series
- **Fixes (2016):**
  - `b1d34de4` (2016-03-14) "Fix inappropriate fetch of entities content"
  - `8f30bdff` (2016-04-15) "Add missing increments of recursion depth counter to XML parser"
  - `bdd66182` (2016-05-23) "Avoid building recursive entities"
  - `9ab01a27`, `c1d1f712` (2016-06-28) XPointer fixes
- **Surface:** entities, recursion, XPointer.
- Many CVE-2016-* tags exist (CVE-2016-1762, CVE-2016-1833..1840,
  CVE-2016-3627, CVE-2016-3705, CVE-2016-4449, CVE-2016-4483).

---

## 3. Later fixes

### SEC-0010 — CVE-2021-3541 (regexp)
- Tag `CVE-2021-3541` exists.
- **Surface:** xmlregexp.

### SEC-0011 — Post-2022 hardening
- 2.15.x NEWS records security fixes: e.g. v2.15.3 (Apr 2026) lists
  "parser: Pass userData to SAX text callbacks in xmlParseReference
  (type-confusion)", "entities: copy children in xmlCopyEntity", and c14n
  double-free fixes. These are recent and must be characterized.

---

## 4. Open security-custody work

- The full CVE list is indexed in `atlas/releases/*/_cve_fix_commits.json`
  (31 libxml2 + 1 libxslt marker tags). Each needs a full SEC- entry with
  trigger, symptoms, root cause, and regression court.
- libxslt security history (e.g. CVE-2015-7995) is not yet characterized.
- The 2.9.0 default-limits break (§ SEC-0001) and the CVE-2014-3660 entity
  regression (§ SEC-0006) are the two highest-priority custody items.

---

## 5. 11.1-V — Security-relevant compatibility verification (2026-08-30)

The candidate's security-sensitive surface was audited and courted against
the system oracle (libxml2 2.15.3 / libxslt 1.1.45). The SECURITY-LIMITS
court (tools/abi/security_limits_probe.py,
courts/suites/data-abi/security-limits-probe.c) compiles one deterministic
C probe twice (oracle + candidate) and requires byte-identical stdout.
Beyond the court's 10 cases, a full amplification-threshold sweep (entity
chains L4..L9 × 10, amplification factors 5..4e9) matches the oracle on
every boundary.

### 5.1 Verified protections (byte-identical with the 2.15.3 oracle)

- **Entity amplification guard** (SEC-0006 / CVE-2014-3660 lineage):
  `xmlParserEntityCheck` semantics — recursive-sum `ent->expandedSize`,
  per-reference accumulation (parent-entity slot / `ctxt->sizeentcopy`),
  `XML_PARSER_ALLOWED_EXPANSION` = 1,000,000, default amplification 5
  (`XML_MAX_AMPLIFICATION_DEFAULT`), `xmlCtxtSetMaxAmplification`,
  fatal `XML_ERR_RESOURCE_LIMIT` (114), no `XML_PARSE_HUGE` bypass.
  Implemented in `src/xml/parser/state.rs::parser_entity_check`.
- **Entity loop detection** (SEC-0004 / CVE-2013-2877 lineage):
  `XML_ENT_EXPANDING` re-entry raises fatal `XML_ERR_ENTITY_LOOP` (89).
- **Parser depth limit**: element nesting capped at 256 (2048 with
  `XML_PARSE_HUGE`) — "Excessive depth in document" with
  `XML_ERR_RESOURCE_LIMIT`, raised from the default SAX handler
  (`src/xml/sax/default.rs`), catastrophic stop (`disableSAX = 2`) exactly
  like upstream `xmlCtxtVErr`.
- **Catastrophic-error stop + 100-error suppression**: `disableSAX = 2`
  for RESOURCE_LIMIT/ENTITY_LOOP; non-catastrophic errors suppressed after
  100 when the document is already not well-formed (`XML_MAX_ERRORS`),
  matching `xmlCtxtVErr`.
- **NONET / XXE**: `xmlCheckHTTPInput` refuses http URLs under
  `XML_PARSE_NONET`; unloadable external entities fail silently (upstream
  `xmlCtxtParseEntity`), not as undeclared-entity errors.
- **Catalog load return**: `xmlLoadCatalog` returns int 0/1 (success/error)
  per upstream catalog.c (the candidate previously returned the handle
  semantics of `xmlCatalogLoad`).

### 5.2 Safe-divergence records

```text
── record SD-001 ────────────────────────────────────────────────────────
historical oracle behavior: pre-2.9.0 libxml2 expanded entity chains with
  no amplification bound; billion-laughs documents expanded fully
  (SEC-0001/SEC-0006; CVE-2014-3660).
security impact: exponential CPU and memory consumption (DoS).
safe divergence: the candidate implements the modern 2.15.3 semantics —
  the amplification guard (factor 5 default, 1M expansion bound,
  xmlCtxtSetMaxAmplification) rejects such documents with
  XML_ERR_RESOURCE_LIMIT; in addition the candidate's entity model parses
  each entity's content once and caches it, so exponential re-expansion is
  structurally impossible even before the guard fires.
externally observable difference: none vs the 2.15.3 oracle — identical
  rejection thresholds across the whole amplification sweep.
reason divergence is mandatory: emulating the vulnerable behavior would
  reintroduce CVE-2014-3660.
──────────────────────────────────────────────────────────────────────────

── record SD-002 ────────────────────────────────────────────────────────
historical oracle behavior: pre-2.9.0 libxml2 had no default parser depth
  or size limits (SEC-0001).
security impact: DoS via deeply nested documents / oversized constructs.
safe divergence: the candidate implements the 2.15.3 limits — element
  depth 256 (2048 with XML_PARSE_HUGE) with XML_ERR_RESOURCE_LIMIT;
  XML_MAX_TEXT_LENGTH 10,000,000; XML_MAX_NAME_LENGTH 50,000.
externally observable difference: none vs the 2.15.3 oracle (deep-nesting
  rejected at 256, accepted at 2048 under HUGE).
reason divergence is mandatory: matching the modern hardened oracle.
──────────────────────────────────────────────────────────────────────────

── record SD-003 ────────────────────────────────────────────────────────
historical oracle behavior: recursive entity declarations (<!ENTITY a
  "&a;">) could loop (SEC-0004; CVE-2013-2877).
security impact: parser hang / stack exhaustion.
safe divergence: XML_ENT_EXPANDING re-entry raises fatal
  XML_ERR_ENTITY_LOOP (89), byte-identical with the 2.15.3 oracle.
externally observable difference: none.
reason divergence is mandatory: loop emulation is unsafe.
──────────────────────────────────────────────────────────────────────────

── record SD-004 ────────────────────────────────────────────────────────
historical oracle behavior: external entity URLs were fetched over the
  network regardless of context (XXE).
security impact: SSRF / file disclosure (classic XXE).
safe divergence: XML_PARSE_NONET is honored (xmlCheckHTTPInput); unloadable
  external entities fail silently — both matching the 2.15.3 oracle.
externally observable difference: none vs the oracle (nonet-entity and
  ext-entity cases byte-identical).
reason divergence is mandatory: XXE emulation is unsafe.
──────────────────────────────────────────────────────────────────────────
```

### 5.3 Fidelity notes (non-safety deviations kept in custody)

- `xmlStringDecodeEntities`/`xmlStringLenDecodeEntities`
  (`src/abi/exports_string.rs::expand_entity_into`) is a documented
  simplified port: the depth-20 / XML_ENT_EXPANDING guards exist but
  errors are silent (the API path is not exercised by the SECURITY-LIMITS
  court; the main parser path carries the full semantics above).
