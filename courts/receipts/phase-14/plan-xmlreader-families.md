# plan-xmlreader — engine root-cause FAMILY map (SP-14.3.5, 29 head tests)

Method: for each `.phpt` in the failing set (head `plan-xmlreader-head.txt`, 29 unique),
read the PHP body AND the captured candidate divergence in
`phpbuild-c:/srcb/php-src/ext/xmlreader/tests/<id>{.diff,.out}`, then probed the live
candidate `php` (same DSO as the receipt run) with byte-tiny reproducers to separate
*engine load-path* bugs from *php-wiring* and from *message-only* differences.
No `.phpt` in this set is left un-inspected (all bodies + diffs read); none is flagged
unverified. No member is an active crash in-receipt (crash-list is dom/xml/xsl), but two
members are **latent crash-guard pins** (gh16292, gh19098) that today "pass" only
vacuously because the reader never runs.

Obs-Severity: O=output/content mismatch; M=message/error parity only; C=would-crash-if-run
(latent). All members are effectively O except ones flagged.

Engine reader model to keep in mind (from `src/xml/reader/mod.rs`): the reader is a
pre-parse whole-buffer tree reader. `xmlTextReaderSetup`/`xmlReaderFor*` slurp/parse into
`r.doc` and fill an `r.events` list (`event_index`,`cur_node`,`attribute_count= -1`,
`cur_attribute`), and `read()`/`next()`/`moveTo*` walk that event cursor. So almost every
family below is an **event-list / cursor** divergence in this module, not in
`php_xmlreader.c` (which is upstream PHP and identical in both builds).

---

## Ordering summary (do in this order; gates explained per family)

1. **NR** — the dominant tie (18 whole members + 4 string-halves): non-file input (memory `XML()/fromString`,
   IO `fromStream`) emits **zero** reader events.
2. **NX** — file reader drops the END_ELEMENT event of a *truly empty* (zero-child)
   element.
3. **AT** — attribute cursor/value record divergence (moveTo*/name/value/getAttribute parity).
4. **EV** — empty attribute value `""` collapses to NULL (sibling of engine empty-string/ptr fixes).
5. **VA / VD** — reader validation attach (RelaxNG/XSD) and reader DTD/LOADDTD/VALIDATE w/ external DTD.
6. **EX** — expand / reader-node lifetime (`php_libxml_node_free`) guards, plus memory-reader
   default base-URI residual.

---

## NR — non-file reader content never yields a first node (biggest family) | O
count = **18 whole-reader members** (blocked immediately), plus the `XML()`/IO string
halves of `007/008/012/013` (also NR, but their real family is further below):
`001`, `006`, `009`, `010`, `011`, `bug42139`, `bug64230`, `cache_slot`,
`expand`, `readString_basic`, `next_basic`, `var_dump`,
`fromString_custom_constructor`, `fromStream_custom_constructor`,
`fromStream_broken_stream`, `gh16292`, `gh19098`, and `static`(string half). (`static`'s *file* half is NX, not NR.)
Root cause (confirmed empirically): every reader built from an **in-memory input buffer**
(`XMLReader::XML()/fromString()` → `xmlNewTextReader`+`xmlTextReaderSetup`, memory buffer
path) or from an **IO stream callback** (`fromStream` → `xmlReaderForIO`) issues **zero**
`xmlTextReaderRead()` events (first read returns 0/EOF, `nodeType` stays 0). The same
`php` binary reads the identical document fine from `open($file)` (`xmlReaderForFile`),
so this is a load-path-specific divergence in `reader/mod.rs` (`xmlTextReaderSetup`,
`xmlReaderForMemory` ~2167, `xmlReaderForIO` ~2307) event-list population vs the file
entry. Visible symptoms across members: empty node streams (`006/009/010/011`,
`readString_basic`, `bug42139`, `gh16292`, `gh19098` empty output), `read()==false` +
"Failed to read property … yet" fatal in `var_dump`/`cache_slot`, `isValid/LOAded-none`.
Prereq for: almost every later family (attribute/EV/validation/expand) that is expressed
only through an `XML()`/IO reader — and for un-masking the two latent-crash pins.
Surface cite (classified, un-fixed): `src/xml/reader/mod.rs` Setup/For* event population.
Overlap: none w/ dom except as it gates expand (EX).

## NX — file reader omits END_ELEMENT of empty no-child element | O
count = 3 file-reader members: `002`, `fromUri_custom_constructor`, `static`(file half).
(Loader scan — 002/fromUri read via `open(file)` of an empty `<books></books>`; static's
`::open` half is the same; its `::XML` half is NR.) Also surfaces as "one read short" in
any empty-doc walker.
Root cause (confirmed empirically): `open($file)` on `<books></books>` emits ELEMENT but
**never END_ELEMENT** for an element with zero child nodes (self-empty open/close, or a
tail-less subtree). When an element has at least one child/text, its END_ELEMENT *does*
appear — so the event-list finalizer drops the END event for empty, child-less elements
(e.g. `readString_basic`/`book` text bodies are fine but a bare `<books></books>` root is
not). Cursor: event-list finalize in `reader/mod.rs` walk.
Order: right after NR (file-based walkers then count fully).

## AT — attribute cursor/record divergence (moveTo*/name,value,getAttribute*) | O
count ≈ **5**. Members: `003`, `003-get-errors`, `003-move-errors`, `003-mb`,
`015-get-errors`(ns). All reach the `<book>` ELEMENT from `open(file)` then diverge on
attribute navigation/reporting.
Root cause: reader attribute position semantics differ from oracle.
  * `moveToNextAttribute()` *from the element position* returns FALSE and leaves the
    cursor on the parent (`name` stays `book`, not `num:1`) — see `003-get-errors.out`
    (`book`/`book:`/bool(false) x3) and `003-move-errors`.
  * `moveToAttribute('num')`/`moveToFirstAttribute()` DO reach `num:1`, but the equality
    `getAttribute(name)==value` flips (candidate prints `…attr failed`; oracle does not),
    i.e. the attribute-node `value`/`getAttribute` strings diverge (whitespace/normalization
    or node-vs-element value retrieval).
  * `moveToNextAttribute` iteration / `getAttributeNs('isbn','uri')` after `next()` (010,
    string NYI) keeps `book:` — namespace-attribute move unresolved.
Engine surface: attribute cursor + node-type/value emission in `reader/mod.rs`
(`MoveToFirstAttribute`/`MoveToNextAttribute`/`MoveToAttribute*` ~2482-2554,
`ConstValue`/`GetAttribute*` ~2919-2997); overlaps the php Attribute name not-empty paths
(those do throw OK — message layer is fine).
Order: after NR+NX.

## EV — empty attribute value `""` reported as NULL | O
count ≈ 1 (canonical sole current member) : `012` (its `XML()`/string half is NR-blocked;
its `open(file)` half). Candidate `getAttribute('bar')` on `<foo bar=""/>` returns NULL
where oracle returns `string(0)""` — present-but-empty attribute collapses to NULL,
echoing the dom `DOMEntity_fields` empty-PUBLIC / b"" dangling-ptr family
(R-14.3-EMPTY-STRING / empty-string-to-NULL) at the attribute-value reader boundary.
Overlap: ext/dom empty-string-value handling; `src/xml/reader/mod.rs` value emission for
empty attribute text. Order: with/just after EV prerequisites (needs element positioned).

## VA — reader schema attach (RelaxNG / XSD) | O/M
count ≈ 2. Members: `007` (setRelaxNGSchema, file+string relaxNG valid), `013`
(setSchema XSD: candidate raises `Schema contains errors` at compile even for the *valid*
input).
Root cause: reader-side validation attach/compile diverges: the RelaxNG schema file is
loaded/`isValid` false when it should be OK (007 file half), and the XSD `013.xsd` fails
to compile through the engine schema front-end (013). The string halves are also NR-gated.
Engine surface: `src/xml/reader/mod.rs` schema/relaxng set-* + schema parse error routing;
overlaps ext/dom `schemaValidate*` (SP-14.3.4 V1/V3) and parser `options.rs` load flags.
Order: after NR (reads) but can proceed in parallel with AT/EV; its message routing family
ties into the dom err-table (do not regress).

## VD — reader DTD/LOADDTD/VALIDATE w/ external DTD | O/M
count ≈ 1: `008` (file DTD validate; string half is NR-gated).
Root cause: reader DTD validation relies on the *external* DTD (`SYSTEM "dtdexample.dtd"` =
LOADDTD+VALIDATE); candidate reports dozens of spurious `No declaration for element TITLE/
ORGTITLE/LOC/…` validity errors on `read()`, i.e. it never loads/attaches the referenced
external DTD, so `isValid` is false (and the run dumps those read() validity warnings).
Oracle loads the DTD, doc validates clean, emits no warnings, `file DTD: ok`.
Engine surface: reader `setParserProp(LOADDTD/VALIDATE)` + external DTD load (internal
subset/external-entity loader); deep overlap with ext/dom validate-external-DTD (SP-14.3.4 V2)
and the parser entity/dtd load family (SP-14.3.1 low-level). Error-text rows echo
`008`'s read() messages -> also a message-parity family; keep true-parity engine concerns.
Order: after NR/NX; group with dom V2.

## EX — expand / reader-node lifetime / php_libxml_node_free (latent crash-guard pins) | C(latent)/O
count ≈ 3 active members: `expand`, `gh16292`, `gh19098` (plus DOM `span` of expand tests).
Root cause: these walk an `XML()`/string reader into `read()/next()` then call
`expand([$domcontext])` and manipulate/take the DOM node (`DOMCharacterData` doc-less base
in gh16292; `expand()->firstChild`, `unset`, `adoptNode($child)` in gh19098). Today they
emit **empty** output purely because NR never runs them; the PHP files are upstream
regression **guards** for segfaults that lived in ext/xmlreader+DOM `php_libxml_node_free`
node lifetime (`expand` in `php_xmlreader.c` `xmlDocCopyNode`; gh19098 title literally
"php_libxml_node_free"). They should be re-run immediately after NR to confirm the
lifetime/free-guard, and a *latent crash* must be watched (kind C) if the engine takes the
previously-segv path.
Overlap: ext/dom O1 `php_libxml_node_free` / adopt-node lifetime; also the memory-reader
default `baseURI` reported as the CWD path `/srcb/php-src/` vs `""` for string sources
(small L2 input-family residual visible in `fromString_custom_constructor.diff`).
Order: last — un-masked only after NR/NX.

---

## Cross-extension / shared-surface flags (per task)
- ext/dom input-encoding & parser-options overlap: VD (reader DTD-load) lands on the same
  external-DTD/`options.rs` load layer as SP-14.3.4 V2 and dom L1/L2; EV's empty-string->NULL
  is the same engine empty-string/ptr pattern the Phase-14 empty-string fixes guard; VA
  message rows tie to dom schema/relaxng err tables (do-not-regress).
- reader-node lifetime / `php_libxml_node_free` overlap: only EX (expand/gh16292/gh19098);
  keep the guard probes so prior dom free-lifetime fixes don't regress the reader path.
- Memory/base-URI input-family (L2-style) — explicit note, not a separate current member:
  memory-string readers report CWD-derived base URI where oracle reports "".

## Receipt / validation note
NR, NX behaviorally reproduced live against the receipt DSO (phpbuild-c) with minimal
probes; oracle (real libxml2 2.15.3) passes all 29 => the divergence is in libxml-rs's
reader event list, not PHP. No code changed; this is a classification map for the
14.3-order planner.
