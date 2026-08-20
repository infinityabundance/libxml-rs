# Lore Archive

Per §11: this documents things a competent developer would **not** learn by
merely reading the current API manual. Every entry needs evidence. Confidence
labels: `CONFIRMED` (directly verified from source/history), `STRONGLY_SUPPORTED`,
`INFERRED`, `UNRESOLVED`.

Source identifiers per `atlas/SOURCES.md` (e.g. `SRC-LIBXML2-GIT`).

---

## A. Parser and options

### LORE-0001 — `XML_PARSE_HUGE` is the only way to lift default parser limits
- **Confidence:** CONFIRMED
- **Evidence:** commit `52d8ade7` (2012-07-30, "Introduce some default parser
  limits") — see `atlas/SECURITY_HISTORY.md` SEC-0001.
- **Behavior:** since 2.9.0, name length, dictionary size, and lookup count are
  bounded by default. Documents that exceed the limits fail with new errors
  unless `XML_PARSE_HUGE` is set.
- **Why unusual:** it changed default behavior for *all* callers, not just
  hostile inputs. Downstream code that parsed large-but-legitimate documents
  broke.
- **Compatibility consequence:** candidate must reproduce the same default
  limits and the same error codes, and honor `XML_PARSE_HUGE`.
- **Courts:** `PARSER-LIMIT-*`.

### LORE-0002 — The upstream spelling `XML_MAX_TEXT_LENGHT`
- **Confidence:** CONFIRMED
- **Evidence:** commit `1fb2e0df` (2009-01-18) — the commit message misspells
  "LENGTH" as "LENGHT".
- **Behavior:** the limit macro was named `XML_MAX_TEXT_LENGHT` (misspelled) in
  the code; later corrected. Historical macros are part of the observable
  surface.
- **Courts:** `API-*` (macro inventory).

### LORE-0003 — `XML_PARSE_HUGE` existed before the default limits
- **Confidence:** CONFIRMED
- **Evidence:** `XML_PARSE_HUGE = 1<<19` present in 2.8.0 parser.h; default
  limits only came in 2.9.0.
- **Behavior:** in 2.8.x, `XML_PARSE_HUGE` was a no-op-ish relaxation; from
  2.9.0 it became functionally meaningful. A historical profile must reflect
  this.

### LORE-0004 — NEWS lagged behind releases (2.7–2.9 era)
- **Confidence:** CONFIRMED
- **Evidence:** `v2.9.1:NEWS` still lists 2.7.6 as the newest entry — the
  auto-generated NEWS (from `http://xmlsoft.org/news.html`) lagged the actual
  release tags.
- **Behavior:** NEWS is not a reliable release-by-release changelog in that
  era; the git history is authoritative.
- **Compatibility consequence:** the atlas must derive per-release semantic
  changes from git history + ChangeLog, not NEWS alone.

## B. Threading and globals

### LORE-0005 — Thread support predates the "thread-local globals" era
- **Confidence:** CONFIRMED
- **Evidence:** `globals.c` thread support integrated 2001-10-12/13
  (commits `b847864f`, `d0463560`).
- **Behavior:** threading was present very early; the big semantic transition
  (per-thread parser state vs global state) happened over a long period. The
  epoch model (`compatibility/profiles.rs`) must capture this.

## C. Tree and memory

### LORE-0006 — Namespace nodes have no parent (long-standing divergence)
- **Confidence:** CONFIRMED (as a long-standing fact upstream was aware of)
- **Evidence:** c14n.c birth commit `044fc6b7` (2002-03-04) message mentions
  "fixing #61290 'namespace nodes have no parent' long standing divergence".
- **Behavior:** `xmlNs` nodes have no parent pointer in the tree; XPath
  namespace nodes behave specially. This is a documented/quirk behavior
  downstream code depends on.
- **Courts:** `XPATH-NS-*`, `TREE-NS-*`.

## D. XPath

### LORE-0007 — XPath number formatting / NaN / signed zero
- **Confidence:** UNRESOLVED (to be differentially tested)
- **Behavior:** XPath 1.0 string(number) has edge cases (negative zero, NaN,
  infinity) where libxml2's behavior must be matched exactly. Listed in
  standards atlas as S-0001.
- **Courts:** `XPATH-NUMBER-*`.

## E. Security-related

### LORE-0008 — CVE fixes that changed observable behavior
- **Confidence:** CONFIRMED
- **Evidence:** CVE-2014-3660 fix `be2a7eda` (2014-10-16) followed by
  regression fix `72a46a51` (2014-10-23). See `atlas/SECURITY_HISTORY.md`.
- **Behavior:** the CVE fix bounded entity expansion and thereby changed
  observable entity semantics; a regression was then fixed. Candidate must
  reproduce the *final* behavior (post-regression-fix), not the pre-CVE
  behavior, and the divergence must be documented.
- **Courts:** `PARSER-ENTITY-*`.

## F. Pending lore (candidate investigations)

These are `UNRESOLVED` and must be investigated with oracle experiments before
any parity claim (§97 evidence loop). Do not manufacture lore.

- `UNRESOLVED`: exact XSLT template-priority corner cases (import precedence,
  `xsl:apply-imports`).
- `UNRESOLVED`: result-tree-fragment (RTF) semantics in libxslt.
- `UNRESOLVED`: libxml2 HTML parser tag-recovery specifics per version.
- `UNRESOLVED`: catalog lookup precedence and `XML_CATALOG_FILES` handling.
- `UNRESOLVED`: serializer default escaping / whitespace preservation.
- `UNRESOLVED`: `xmlGetLineNo` edge cases (line-number attribution).
- `UNRESOLVED`: dictionary ownership quirks (`XML_DICT_*` behavior).
- `UNRESOLVED`: `xmlSetEntityLoader` and resource-loader transitions.
- `UNRESOLVED`: entity substitution surprises (`XML_PARSE_NOENT`).
- `UNRESOLVED`: `xmlNodeDump`/serializer recursion and formatting.
- `UNRESOLVED`: EXSLT `date:` quirks.

---

## Maintenance

Every lore entry must eventually carry:
- evidence (source identifier + verified fact)
- confidence label
- court that protects it
- whether it is `CONFIRMED`/`STRONGLY_SUPPORTED`/`INFERRED`/`UNRESOLVED`

When a `UNRESOLVED` item is investigated, promote it with its oracle result
and link its court + receipt.
