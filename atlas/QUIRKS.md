# Quirk Registry

Per §72: a separate registry of **confirmed** compatibility quirks. This is one
of the most valuable outputs of the project. Each quirk records subsystem,
versions, behavior, standards relation, discovery, upstream evidence,
downstream dependence, candidate handling, and regression courts.

Entries are only added once confirmed by source evidence or differential
courts. Pending investigations live in `atlas/LORE.md` under `UNRESOLVED`.

---

## QUIRK-0001 — Default parser limits changed behavior in 2.9.0
- **Subsystem:** Parser / options
- **Versions:** behavior since 2.9.0; `XML_PARSE_HUGE` since 2.8.0
- **Behavior:** name-length, dictionary-size, and lookup-count limits are
  enforced by default; `XML_PARSE_HUGE` lifts them
- **Standards relation:** n/a (DoS hardening)
- **Discovery:** commit `52d8ade7` (2012-07-30)
- **Upstream evidence:** `SRC-LIBXML2-GIT`, `atlas/SECURITY_HISTORY.md` SEC-0001
- **Downstream dependence:** large-document consumers set `XML_PARSE_HUGE`
- **Candidate handling:** reproduce default limits + `XML_PARSE_HUGE`
- **Regression courts:** `PARSER-LIMIT-*`

## QUIRK-0002 — Namespace nodes have no parent
- **Subsystem:** Tree / XPath
- **Versions:** all (long-standing)
- **Behavior:** `xmlNs` nodes have no parent pointer; XPath namespace-node
  semantics differ from element nodes
- **Standards relation:** XPath 1.0 namespace-axis semantics
- **Discovery:** upstream fix `044fc6b7` (2002-03-04) for #61290
- **Upstream evidence:** `SRC-LIBXML2-GIT`
- **Downstream dependence:** XPath namespace-axis consumers
- **Candidate handling:** model namespace nodes distinctly
- **Regression courts:** `XPATH-NS-*`, `TREE-NS-*`

## QUIRK-0003 — NEWS file lagged releases (2.7–2.9 era)
- **Subsystem:** Documentation / release process
- **Versions:** ~2.7–2.9
- **Behavior:** NEWS auto-generated and lagging; not a reliable per-release
  changelog
- **Standards relation:** n/a
- **Discovery:** `v2.9.1:NEWS` still listed 2.7.6 as newest
- **Upstream evidence:** `SRC-LIBXML2-GIT`
- **Downstream dependence:** none (archaeological)
- **Candidate handling:** use git history, not NEWS, for delta atlas
- **Regression courts:** `HIST-*`

## QUIRK-0004 — `XML_MAX_TEXT_LENGHT` misspelled macro
- **Subsystem:** Parser / macros
- **Versions:** 2.9 era
- **Behavior:** the limit macro was named `XML_MAX_TEXT_LENGHT` (typo); later
  corrected. Historical macros are observable surface.
- **Standards relation:** n/a
- **Discovery:** commit `1fb2e0df` (2009-01-18)
- **Upstream evidence:** `SRC-LIBXML2-GIT`
- **Downstream dependence:** code compiled against the old macro name
- **Candidate handling:** preserve historical macro names in header atlas
- **Regression courts:** `API-*`
