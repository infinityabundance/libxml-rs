# Semantic Epochs — libxml2/libxslt Historical Behavior Matrix

> **Phase 11 deliverable (§85):** *"The project can explain how current behavior came to exist."*
> Method: §41 (historical oracle matrix), §42 (version fingerprints), §51 (multi-version triangulation).

This atlas records the **semantic epochs** of libxml2/libxslt observable behavior as
measured by the historical oracle matrix under `oracle/historical/`. An epoch is a
maximal set of consecutive releases that produce **byte-identical** observable output
(stdout + stderr + exit status) for a given behavioral case. Epoch boundaries are
correlated with upstream commits/NEWS entries so the current behavior can be traced
to the exact change that created it.

All results are reproducible:

```sh
# 1. build an oracle from the archaeology git clone (era-autotools modernization included)
oracle/historical/build.sh libxml2 2.13.0
# 2. run the full behavioral matrix across every built oracle + the system oracle
oracle/historical/run_matrix.sh all   # writes oracle/historical/results/{matrix.json,fingerprints}
```

---

## 1. Oracle registry (§42 — every oracle proves its identity)

Each oracle is identified by its own `--version` output captured as the `version` case
in the matrix (all 13 libxml2 `version` hashes distinct, all 6 libxslt `version` hashes
distinct — no oracle is unidentified). Per-version `fingerprint.json` files record the
sha256 of every case's (exit, stdout, stderr) triple.

| Project | Versions built from archaeology git | System oracle |
|---|---|---|
| libxml2 | 2.7.8, 2.8.0, 2.9.4, 2.9.10, 2.9.14, 2.10.4, 2.11.5, 2.12.6, 2.13.0, 2.13.5, 2.14.1, 2.15.0 | 2.15.3 (`21503-GITv2.15.3`) |
| libxslt | 1.1.26 (↔libxml2-2.7.8), 1.1.32 (↔2.9.4), 1.1.35 (↔2.9.10), 1.1.38 (↔2.10.4), 1.1.42 (↔2.11.5) | 1.1.45 (`10145-GITv1.1.45`) |

Older versions: 2.6.32 and earlier require a period-correct autotools toolchain and
glibc (modern glibc rejects the era's `threads.c`); 2.7.8 is the oldest anchor
buildable on this host. A tier-3 container would be required for the 2.0–2.6 span
(see `oracle/historical/build.sh` for the recorded era adaptations).

Build identity: GCC 16.2.1, autoconf 2.73, automake 1.18.1; matrix runner: bash +
python3; capture of a full run is `sha256(oracle/historical/results/matrix.json)` =
`b270a9fb360987be70d22f33df238d9f9c99631e44c7744b42766471ab8b6141` (2026-08-29 run,
11.1-A). Every oracle additionally carries a committed `oracle-manifest.json`
(`atlas/oracle-manifests/`) binding upstream tag/commit SHA, source-tree hash,
adaptation-script hash, host/libc/arch, compiler/autotools versions, configure
argv, feature manifest, config-header hash and built binary/library hashes; the
matrix hashes all 18 manifests in `matrix.json["_oracle_manifests"]`.

---

## 2. Epoch map (per-case version groups with identical output)

Legend: `{A → B}` = all libxml2 versions between A and B inclusive are byte-identical;
`2.9.10*` = single-version group.

| Case (corpus fixture) | Epoch A | Epoch B | Epoch C | Surface |
|---|---|---|---|---|
| `xpath-nodeset`, `xpath-count`, `xpath-string` (`lib.xml`) | `{2.7.8 → 2.9.4}` | `{2.9.10 → 2.15.3}` | — | xmllint XPath dump serialization |
| `xpath-attr` (`lib.xml --xpath '@id'`) | `{2.7.8 → 2.10.4}` exit 10 | `{2.11.0 → 2.11.5}` exit 0 | `{2.12.6 → 2.15.3}` exit 11 | xmllint XPath empty-set handling |
| `parse-error` (`bad.xml`) | `{2.7.8 → 2.11.5}` two diagnostics, exit 1 | `{2.9.10}` regression diagnostic | `{2.12.6}` single diagnostic, exit 1; `{2.13.0 → 2.15.3}` exit 4 | parser diagnostics + xmllint exit |
| `recover-bad` (`--recover bad.xml`) | `{2.7.8 → 2.11.5}` | `{2.9.10}` regression | `{2.12.6 → 2.15.3}` | parser recovery diagnostics |
| `undeclared` (`undeclared.xml`) | `{2.7.8 → 2.12.6}` exit 1 | `{2.13.0 → 2.15.3}` exit 4 | — | parser diagnostics + exit |
| `attr-markup-entity` (`markattr.xml`) | `{2.7.8 → 2.12.6}` reported once, exit 1 | `{2.13.0 → 2.15.3}` reported twice, exit 4 | — | entity-in-attribute fatal error |
| `noent-debug` (`--debug --noent dclent.xml`) | `{2.7.8 → 2.12.6}` `TEXT` | `{2.13.0 → 2.15.3}` `TEXT compact` | — | entity-decl content node storage |
| `valid-invalid` (`--valid invalid.xml`) | `{2.7.8 → 2.12.6}` exit 4 | `{2.13.0 → 2.15.3}` exit 3 | — | DTD validation exit |
| `valid-nodtd` (`--valid simple.xml`) | `{2.7.8 → 2.12.6}` exit 4 | `{2.13.0 → 2.14.1}` exit 3 | `{2.15.0 → 2.15.3}` exit 0 | no-DTD validation exit |
| `html-dump` (`--html page.html`) | `{2.7.8 → 2.14.1}` formatted (newlines) | `{2.15.0 → 2.15.3}` single line | — | HTML serializer |
| `xsltproc basic/num/empty` | `{1.1.26 → 1.1.45}` **byte-identical, no epoch split** | — | — | XSLT transform output |

**Stable cases** (identical across the entire 2.7.8 → 2.15.3 span, no epoch split):
`dump-simple`, `dump-empty`, `dump-dtd`, `format-dtd`, `noent`, `noent-decl`,
`attr-entity`, `c14n`, `copy-dtd`, `dropdtd`, `debug-simple/dtd/nodes/longtext/ns`,
`html-debug`, and the compact-text threshold (text of 14/15 bytes dumps `TEXT compact`,
16+ bytes dumps `TEXT`, unchanged in every version).

---

## 3. Epoch findings and their upstream provenance (§51 correlation)

### E-001 — `xmllint --xpath` node-set output: concatenated → newline-separated

- **Boundary:** first changed in **2.9.10** (2.9.4 concatenated `T1T2`; 2.9.10+ prints one node per line).
- **Commit:** `da35eeae5b92b88d8ebdb64b4b327ac1c2cf1bce` — Nick Wellnhofer,
  *"Add newlines to 'xmllint --xpath' output"*, 2018-09-23.
- **Commit message evidence:** *"Separate nodes in a node-set with newlines and always
  add a terminating newline. This is a breaking change but the old behavior of dumping
  text nodes without separator was mostly useless."* — an upstream-documented breaking change.
- **Current behavior explains:** the crate targets 2.15.3, i.e. the newline-separated epoch.

### E-002 — parse-error second diagnostic: "Premature end" → 2.9.10 regression "EndTag: '</' not found" → dropped (2.12)

- **Regression window:** exactly **2.9.10** (`bad.xml` yields
  `parser error : EndTag: '</' not found` instead of `Premature end of data in tag a line 1`),
  introduced by the 2.9.10 non-recursive parser refactor
  (`62150ed2` "Make xmlParseContent and xmlParseElement non-recursive" era).
- **Fix:** `de5b624f10e9d29ff1b3bbc07358774a3725898e` — *"Fix handling of unexpected EOF in
  xmlParseContent"*, 2021-05-08, first release **2.9.11**.
- **Second boundary:** 2.12.x parser error-handling rework (`c6083a32` "parser: Improve
  error handling in push parser" et al., NEWS 2.12.0) **dropped the second diagnostic
  entirely**; only the mismatch line remains, then the exit code changes 1 → 4 in 2.13.0
  (E-005). The crate matches the 2.12.6+ diagnostic epoch and the 2.13+ exit epoch.

### E-003 — `xpath-attr` empty node-set exit code: 10 → 0 → 11

- **10 → 0:** `e85f9b98` *"xmllint: Improve handling of empty XPath node sets"*, first release **2.11.0**.
- **0 → 11:** `387a952b` *"xmllint: Return error code if XPath returns empty nodeset"*, first release **2.12.6**.
- Message (`XPath set is empty`) is byte-identical in all versions — the epoch is purely the exit code.

### E-004 — entity content node in `--debug --noent` dumps: `TEXT` → `TEXT compact`

- **Boundary:** **2.13.0** (2.12.6 and earlier dump the entity content child as `TEXT`;
  2.13.0+ dump `TEXT compact`).
- **Commit:** `8d04f0eea0a7ca1c8c4c4fe992904f680ba9d7ad` — *"tree: Refactor text node updates"*,
  2024-03-11, first release **2.13.0**.
- **Residual relevance:** R-000119 (entity content children in `--debug`) — the crate's
  synthesized `TEXT compact` node for plain entity content matches the **2.13.0+ epoch**,
  i.e. the current system oracle, not the pre-2.13 behavior.

### E-005 — parser-error/validation exit codes reworked (2.13.0)

- **Boundary:** **2.13.0** for `parse-error`/`undeclared`/`attr-markup-entity`
  (exit 1 → 4), `valid-invalid` (exit 4 → 3), `valid-nodtd` (exit 4 → 3).
- **Correlation:** NEWS 2.13.0 "xmllint: Rework parsing" / "xmllint: Clean up option
  handling"; error-reporting consolidation commits (`b717abdd` "parser: Consolidate error
  handling for undeclared entities", `e8fb3d63` "parser: Convert some 'internal errors' to
  meaningful codes").
- **attr-markup-entity double-report:** `XML_ERR_LT_IN_ATTRIBUTE` ("'<' in entity ... is not
  allowed in attributes values") is raised from multiple parser sites; from 2.13.0 the
  fatal error is reported **twice** (parser + follow-up path) with exit 4, where pre-2.13
  it was reported once with exit 1. The crate reports once (pre-2.13 count) with exit 4
  (2.13+ exit) — a hybrid; see R-000121.

### E-006 — `--valid` with no DTD: exit 3 → 0 (2.15.0)

- **Boundary:** **2.15.0** (2.13.0–2.14.1 exit 3; 2.15.0+ exit 0). Output bytes identical
  in every version — exit code only.
- **Correlation:** 2.15.0 xmllint refactor (NEWS 2.15.0: "Parts of the xmllint executable
  were refactored, allowing the combination of more options"); the "no DTD found"
  validation failure stopped being an exit-worthy error. Exact commit not isolated.

### E-007 — HTML serialization: formatted → single-line (2.15.0)

- **Boundary:** **2.15.0** (2.14.1 and earlier `xmllint --html page.html` emits newlines
  after elements; 2.15.0+ emits one line).
- **Mechanism:** 2.14.1's `HTMLtree.c` contains six `xmlOutputBufferWriteString(buf, "\n")`
  calls in the dump path (lines 697/759/772/854/867/884); **all six were removed** during
  the 2.15.0 serializer rework (`0d81d6f8` "html: Use xmlOutputBufferWrite if possible",
  `46f05ea4` "html: Rework meta charset handling", et al.).
- **Current behavior explains:** the crate's single-line `--html` output is the 2.15.0+ epoch.

### E-008 — libxslt core transform output: stable epoch (no change in 15 years)

- The `xsltproc` cases (`basic`, `num`, `empty`) are **byte-identical** from
  libxslt 1.1.26 (2009) through 1.1.42 and the system 1.1.45 — a fully stable epoch.
- Implication: XSLT 1.0 transform output has been frozen since at least 2009; the crate's
  XSLT engine targets this invariant, and any modern residual is a candidate bug rather
  than an epoch difference.

---

## 4. How this explains current behavior (deliverable summary)

Every behavior of the crate targeting system libxml2 2.15.3 / libxslt 1.1.45 can be
placed in the epoch it was inherited from:

| Current (2.15.3) behavior | Created in | Change |
|---|---|---|
| `--xpath` node-set: one node per line | 2.9.10 | da35eeae (documented breaking change) |
| parse-error: single diagnostic | 2.12.x | error-handling rework (c6083a32 et al.) |
| parse-error / undeclared / entity-in-attr: exit 4 | 2.13.0 | xmllint error rework |
| entity-attr fatal error reported twice | 2.13.0 | parser error consolidation |
| entity-content debug node: `TEXT compact` | 2.13.0 | 8d04f0ee |
| `--valid` invalid: exit 3 | 2.13.0 | xmllint validation exit rework |
| `--valid` no-DTD: exit 0 | 2.15.0 | xmllint refactor |
| `--html` dump: single line | 2.15.0 | HTML serializer newline removal |
| xsltproc transform bytes | unchanged since ≤2009 | stable epoch |

## 5. Court artifacts

- Matrix + fingerprints: `oracle/historical/results/` (regenerable; gitignored).
- Casefiles: `courts/suites/historical/HIST-EPOCH-*.json` (§43).
- Receipt: `courts/receipts/historical-matrix-*.json` (§44).
- Residual triangulation updates: `atlas/RESIDUAL_LEDGER.md` (R-000119, R-000120, R-000121).
