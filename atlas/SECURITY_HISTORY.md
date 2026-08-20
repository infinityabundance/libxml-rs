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
