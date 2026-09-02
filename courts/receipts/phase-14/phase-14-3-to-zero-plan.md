# Phase 14.3 — plan to ZERO failures (289 → 0)

## Progress
- **KEY-3 closed (2026-09-02):** PI-vs-XML-decl routing + reserved-name /
  not-finished codes. ext/xml 1 → **0** — SP-14.3.1 fully closed. Full suite
  **276 → 275**, zero regressions. Receipt: php-14-3-pi-decl-routing-20260902/.
- **KEY-2 + SP-14.3.1-8 closed (2026-09-02):** content-`<!`-markup rule
  (ENTITY/DOCTYPE/ELEMENT-in-content → NAME_REQUIRED 68 + wf=0) + the
  push/default-markup closure (sync_input_position live-buffer repoint,
  EOF-in-construct pause). Full suite **283 → 276** (dom 169 → 166, xml 5 → 1:
  bug27908 + bug46699 + gh20439_1 + gh20439_2 PASS; xml_error_string =
  SP-14.3.1-9 remains), zero regressions. Receipt:
  php-14-3-sp8-content-markup-20260902/.
- **KEY-1 closed (2026-09-02):** BOM-less declared encoding (ISO-8859-1)
  transcoded to UTF-8. Full suite **289 → 283** (ext/xsl 58 → 52, −6: xslt001 +
  xsltprocessor_{get,remove}Parameter{[,-invalidparam]} + setparameter-nostring),
  zero regressions. Receipt:
  php-14-3-key1-declared-encoding-20260902/. Guards: latin1 transcode
  (input.rs) + parser end-to-end (tests.rs).

Authoritative baseline captured at **f190faeb (SP-14.3.1-7)**, full six-extension
`make test` log `phpbuild-c:/out/PLAN-full-cand-head.log`:
**1291 tests / 289 failed / 40 skipped**. Oracle baseline = 0 failed (libxml2
2.15.3 + libxslt 1.1.45 on the pinned PHP 8.5.10).

Split at 289 (now 275 after KEY-1/KEY-2/SP-14.3.1-8/KEY-3):

| ext | head | | ext | head |
|---|---|---|---|---|
| ext/dom | 169 -> 166 | | ext/xml | 5 -> **0** |
| ext/xsl | 58 -> 52 | | ext/simplexml | 9 |
| ext/xmlreader | 29 | | ext/xmlwriter | 19 |

## Sequencing principle (do NOT follow extension boundary)

A full `.phpt`-by-`.phpt` classification (saved alongside: `plan-dom-families.md`,
`plan-xsl-families.md`, `plan-xmlreader-families.md`, `plan-xmlwriter-families.md`,
`plan-simplexml-families.md`) shows the failures are driven by a handful of
**cross-cutting engine rules**, each fanning across several extensions. Order the
work by engine root-cause fan-in and dependency, not by which `ext/` owns the
test. Two rules always hold:

1. **Crash / ownership / double-free families must land first.** A failed
   `free()`/SIGSEGV aborts the whole run and masks every later assertion. Each
   is also a *guard pin*: re-run it after every other keystone.
2. **A shared engine change must not regress a sibling extension.** After any
   edit to `xml/parser` (tokenizer/state/helpers/input), `xml/save`,
   `xml/xpath`, `xml/schema`, `xml/relaxng`, `xslt/transform`, or
   namespace-reconciliation, re-run the owning *and* sibling suites (see the
   gate matrix at the bottom).

### Overarching risk (must be stated up front)
Not every remaining failure is reachable by logic fixes alone. The largest item
by real scope is **R-000157: the crate has no iconv/ICU backend**, so iconv-only
encodings (ISO-8859-2..16, windows-1252, Shift_JIS, EUC-JP, UCS-2/4, EBCDIC,
ISO-2022-JP) report `XML_ERR_UNSUPPORTED_ENCODING` where the oracle returns a
converter. This is a large, self-contained implementation workstream and is
tracked separately (see Workstream 9). Only the *native* set (UTF-8/16LE/BE,
ISO-8859-1, US-ASCII) is converter-parity today. Several failing tests that
*look* like serializer/read errors are actually anchored on this gap.

---

## KEYS. Cross-cutting "keystone" engine rules, in land order

Each keystone has an estimated net effect and a hard gate. Land them one at a
time, commit, full remeasure at the end of KEYS before Extension Work.

### KEY-0 · Crash-ownership foundation — severities/aborts that mask everything
Dom O1 (18, `DOMNode_isEqualNode`, `adoptNode`, `replaceChildren`,
`insertAdjacentElement`, `saveXML_XML_SAVE_NO_DECL`, xpath-fn lifetimes,
`gh22570` …), xmlwriter W6 (bug71536/bug79029), xmlreader latent EX
(`expand`, `gh16292`, `gh19098`), xsl H0 (`bug71571_b` segv,
`bug26384` char-panic), and the `php_libxml_node_free`/delayed-freeing family.
Many dom serialize/ns tests only *reach* their assertion once the underlying node
free is fixed, so this unlocks more than its own count. Ship each owner area as
an isolated commit with its guard probe. **Gate: no `free(): double free`, no
`Segmentation fault`, no `Aborted` anywhere in the six suites; every pin green.**

### KEY-1 · Declared-encoding on BOM-less input streams (parser)
Confirmed live: a BOM-less byte stream with `<?xml encoding="iso-8859-1"?>` and a
`0xE4` byte is REJECTED by the candidate ("Invalid bytes in character encoding")
but loads cleanly on the oracle. Root: input-encoding detection does not honor
the `encoding=` declaration unless a BOM or explicit converter is present;
for non-UTF-8 declared docs the bytes are misread as UTF-8. This is *not* the
R-000157 iconv gap (ISO-8859-1 is in the native set); it is a *detection*
defect.
- Drives 5/57 xsl (`xsltprocessor_{get,remove}Parameter{[,-invalidparam]}` +
  `setparameter-nostring`) *by itself*, and is the leading spurious-warning
  preamble in ~27 xsl diffs.
- Latent over dom (override_encoding/fromFile/fromString, `load_variation*`),
  simplexml, xmlreader.
- **Gate (fan-in measured live):** rerun ext/xsl, ext/dom, ext/simplexml,
  ext/xml. Expect xsl −5 immediately and a measurable cut in the
  warning-preamble class before the real XSLT-engine fix is even started.
- Guard: load a BOM-less `encoding="iso-8859-1"` doc byte-identical to oracle
  (see `lt-eof`/`frag` notes; add a dedicated probe).

### KEY-2 · XML-fragment content-`<!`-markup rule == SP-14.3.1-8 interlock
The parser must clear `wellFormed` (err 68 `XML_ERR_NAME_REQUIRED`) when a
`<!` that is **not** `<!--`, `<![CDATA[`, or a legal `<!DOCTYPE>` in its legal
position appears in **element/content context** — mirroring upstream
`xmlParseStartTag`. Today the engine silently swallows `<!ENTITY …>/<!DOCTYPE`
content as text (`wellFormed` stays 1).
- This is the **genuine closure of SP-14.3.1-8** (gh20439_1/2 + bug27908 in
  ext/xml) that ALSO unblocks dom F1 (`Element_innerHTML_writing`,
  `Element_outerHTML_writing`, `insertAdjacentHTML`, `_reading`,
  `innerHTML_prefixed_writing`, html inner/outer) without regression. A prior
  SP-14.3.1-8 tokenizer-only change (push `<`-at-EOF) passed ext/xml but
  regressed these two dom fragment tests and was reverted (see
  `php-14-3-sp8-gh20439-analysis.md`). This rule is the non-fragile fix for both.
- Land it at the engine content layer, keep DTD/internal-subset decl parsing
  intact, keep prolog `<!DOCTYPE>` legal.
- **Gate:** ext/xml 5 → 0 targets (bug27908+gh20439_1/2 pass) AND dom F1
  7-8 members pass, AND `xml_error_string_basic_libxml` untouched (its family is
  separate), zero regressions in xmlreader/simplexml.

### KEY-3 · Error-code / diagnostic-message-core parity (SP-14.3.1-9 seed)
`xml_error_string_basic_libxml` (ext/xml, rows 47/64 for PI-not-finished and
reserved-name) is the entry point. The message/`errNo` tables mirror into
error-message-text families across dom E1 (name/content-model/version),
xsl messages, xmlreader VA/VD, relaxNG/xsd (V1-V3), xpath "XPath error : "
double-prefix (dom E1 + simplexml 008).
- Phrase this as **one message-harness rule + one table**, then fix consumers to
  route through it, instead of patching each test string.
- **Gate:** `xml_error_string_basic_libxml` PASS; xsl/various message-only rows
  flip with zero logic regressions.

### KEY-4 · RECOVER / NO_XXE / NOENT parse-option net (dom L1 + simplexml S1/S2 +
reader VD)
Shared options flow (`src/xml/parser/options.rs` net): LIBXML_RECOVER recovery
diagnostics suppression, LIBXML_NO_XXE must also block substitution of a
*declared* external general entity + fatal on fetch, NOENT attribute substitution
parity, external-DTD attach/validate. Owns: dom `xml_parsing_LIBXML_{RECOVER,NO_XXE}`
+ `loadHTMLfile*` + `bug80268_2`, simplexml `xml_parsing_*`, reader 008.
- **Gate:** those members PASS, zero regressions in xml/xsl/dom serializers.

### KEY-5 · Namespace / prefix reconciliation + forced fresh-prefix allocation (dom N1)
When an attr whose ns URI is already bound to a *different* prefix is
re-imported/set, allocate a fresh available prefix (e.g. `default##`) instead of
re-declaring a duplicate shadowed `xmlns:`; propagate `namespaceURI` correctly;
Namespace-Error on prefix→conflicting-URI. Gates the HTML-ns serializer,
clone/import, serialize_empty_xmlns and the whole S1-html subtree.
- **Gate:** `createAttributeNS_prefix_conflicts/*`, `import_attribute_namespace`,
  `clone_attribute_namespace_01/02`, `serialize*/_empty_xmlns` green.

### KEY-6 · DTD / entity / notation declaration model (dom D1 + simplexml + reader)
Internal-subset content-model ` , ` retention, `publicId`/`"` ↔ NULL,
DTDNamedNodeMap/baseURI, notation/element/entity decl fidelity off the
SP-14.3.1-1 SAX decl work. Owns DOMEntity_fields, delayed_freeing/**,
dom001/dom005 DOCTYPE serialization, xmlwriter DTD (008/OO_008).
- **Gate:** DOMEntity_fields + delayed_freeing/** + dom001/005 pass.

### KEY-7 · xmlsave / serializer hardening (dom S1/S2, xmlwriter W1..W5,
reader NX, xsl serialization)
Empty-element `<x/>` vs `<x></x>`, `standalone=yes` retention, NO_DECL,
doctype-duplication on cloned docs, `&#xN;`/`&nbsp;` entity round-trip, html
output indentation & ns-import serialization, xmlwriter element-stack prefix
latch, indentation atomicity on PIs/CDATA, write-to-shift_jis transcode.
- **Gate:** per-sub-family member flip with zero regressions.

---

## EXT. Extension engine families (after the KEYS that unblock them)

### EXT-1 ext/xml — finish SP-14.3.1
Subject to KEY-2/3. Sequence: KEY-2 closes SP-14.3.1-8 → SP-14.3.1-9 via KEY-3
→ re-audit SP-14.3.1-10 (`xml_set_object_multiple_times*`, bug46699) which are
provisionally green at HEAD. **Gate: ext/xml = 0.**

### EXT-2 ext/simplexml — 9
Order by the family doc (clone-detach, NO_XXE/RECOVER via KEY-4, empty-string↔`<x></x>`,
`&#38;` decode asymmetry, xpath double-prefix via KEY-3, PI-whitespace trimming,
and gh17153 which belongs to xsl result-ownership — handle under EXT-5).
**Gate: ext/simplexml = 0.**

### EXT-3 ext/dom — drives most of the count
Follow the dom family order (O1 → F1 → N1 → D1 → L2/L1 → S1/S2 → M1 → V → E1)
which the keystones KEY-0/2/5/6/4/7 directly execute. dom is the bulk (169) and
the *last* family (V1-V3 schema/relaxNG, E1 message text) is the deepest.
**Gate: ext/dom = 0.**

### EXT-4 ext/xmlreader — 29
Reader is currently an event-list/cursor walker (`src/xml/reader/mod.rs`):
NR (memory/IO readers emit zero events) → NX (file reader drops END_ELEMENT of
empty no-child elements) → AT/EV (attribute cursor + empty `""`→NULL) →
VA/VD (schema/relaxNG + DTD attach via KEY-4) → EX latent crash-guards (rerun
after KEY-0). **Gate: ext/xmlreader = 0.**

### EXT-5 ext/xsl — 58
Real XSLT/XPath-engine order (NOT pixed by KEY-1 alone): F1 php:function /
registerPHPFunctions / registerPHPFunctionNS routing (~24 members) → F2 EXSLT
dates-and-times + override namespace (`xsltprocessor_exsl_registerPhpFunctionNs`,
`xslt010_gt10129`) → P1 parameter value re-quoting/binding (`bug64137`,
`bug48221`, `setparameter-errorquote`, `req30622`) → F-doc transformTo* result
doc / ownership (`transformToDoc_sxe_type_error`, `_Doc`, `_URI`, `_XML`) →
XLOAD encoding preamble is KEY-1 → H0 crash guards (bug71571, bug26384, prev.
KEY-0) → M1/E1/I1/XD engine edges + the `xslt001-007,012` table/apply divergence.
**Gate: ext/xsl = 0.**

### EXT-6 ext/xmlwriter — 19
Follow the family order (stream/ownership → element-stack prefix + empty/indent
→ DTD → leaf atomicity → shift_jis output). Internal to the writer; independent
of serializer except W5. **Gate: ext/xmlwriter = 0.**

---

## Underway / parallel workstreams (not a sequential phase)
- **W9 · R-000157 iconv/ICU backend** (the real cross-cutting encoding gap;
  also the `$dom->encoding`/shift_jis/xmlwriter W5/xmlreader/dom override_encoding
  tails). Independent implementable in parallel; required for the final stretch.
- **W10 · closure sweep + binary-sub / ZTS / full-gate** after 0 (the existing
  SP-14.3.8 / 14.3-Q / 14.3-S / 14.3-T exits).

## Gate matrix (run after every committed keystone)
| Engine area touched | Rerun (must be green and count-monotonic down) |
|---|---|
| xml/parser tokenizer/state/helpers/input | ext/xml, ext/dom F1, ext/simplexml, xmlreader; keep bug81351/XML_OPTION_PARSE_HUGE/gh12254 green |
| encoding / xmlIO | ext/dom L2, xmlreader, xmlwriter W5, ext/xsl xload, ext/simplexml |
| xml/save + serializer | ext/dom S1/S2, xmlwriter, xsl transformTo*, simplexml |
| namespace reconcile | all dom N1/M1 + clone/import + html serializer |
| xpath / function table | dom L3/O1, xsl F1/F2/M1, simplexml 008 |
| DTD/entity/schema/relaxng | dom D1/V1-V3/E1, reader VA/VD, xmlwriter DTD, xsl schedule |
| ownership/free | ALL (crash-guard pins) |

Tracking + receipts: update `courts/receipts/phase-14/php-14-3-worklist.md`
(fold these family atoms into it) and `CURRENT-STATE.md` on every closure; full
six-extension remeasure + net-count drop required at the end of each KEY and each
EXT.
