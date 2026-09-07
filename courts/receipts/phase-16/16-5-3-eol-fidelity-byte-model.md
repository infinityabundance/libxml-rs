# Phase 16.5.3 — byte-model prerequisite: XML §2.11/§3.3.3 EOL fidelity + DTD-path ASan fixes

Commit: (to be filled)
Date: 2026-09-07
Phase: 16.5.3 (token span architecture — byte-model prerequisite)

## What this change is

Before text runs can be carried as spans of the source buffer (§16.5.3), the
delivered bytes must be **exactly** the source bytes. Oracle A/B probing
(node-level, libxml2 2.15.3 frozen oracle) exposed three pre-existing
fidelity defects where the candidate's delivered bytes diverged from the
oracle on CRLF/CR content:

### 1. Missing XML §2.11 End-of-Line normalization (decode layer)
Upstream `parserInternals.c xmlCurrentChar` substitutes every literal `\r`
(standalone or `\r\n`) with a single `\n` at character-decode time — so ALL
parsed content (element text, CDATA, comments, PI data, attribute values,
entity re-parses) receives `\n`, never raw `\r`. The candidate previously
delivered the raw `\r` and *dropped* the `\n` of CRLF pairs (its position
tracking consumed the pair, but only the `\r` was pushed into content).

Verified divergence (oracle `--format`/node probe vs candidate):
- `<a>one\r\ntwo\rthree\nfour</a>` → oracle text `one\ntwo\nthree\nfour`;
  candidate text `one\rtwo\rthree\nfour`.
- Comments/CDATA/PI data lost the LF of every CRLF pair.

Fix: `InputBuffer::peek_char_inner` returns `Some('\n')` for a source `\r`
(the substitution happens at the same layer as upstream, for every decoded
character consumer). Raw reads (`peek_raw`/`skip_raw_bytes`) and source
windows keep the true source bytes. Position advancement was already
source-byte driven (`advance_past_char`) and already consumed the CRLF pair,
so line/col parity is unchanged — only the delivered character changed.

### 2. Attribute-value literal whitespace → single space (XML §3.3.3)
Upstream `xmlParseAttValueInternal` converts every literal `\t`/`\n`/`\r`
(incl. CRLF) inside an attribute value into exactly one `#x20` for EVERY
attribute type; literal `#x20` runs survive (CDATA attrs keep them raw), and
`&#9;`/`&#10;`/`&#13;` character references survive as literal control
chars. The candidate previously kept raw control bytes in CDATA/undeclared
attribute values (`v\rw` → `v&#13;w`, oracle: `v w`).

Fix: in `scan_attr_value_inner`, a decoded `\t`/`\n` (CR already delivered
as `\n` by fix 1) is emitted as `' '`. The non-CDATA collapse/trim in
`substitute_refs` is unchanged and now operates on the already-converted
spaces, reproducing the same net result.

### 3. DOCTYPE body must transport raw bytes (entity literal values keep CRLF)
The internal-subset body is *transport* for `parse_dtd`; upstream scans it
with raw-byte macros and stores entity/notation literal values with raw CRLF
(`<!ENTITY e "ent\r\nval">` stores `ent\r\nval`), while NOENT expansion of
`&e;` re-parses the value as parsed content where EOL normalization applies
(`ent\nval`). The candidate's per-char decode capture had dropped the LF of
every CRLF pair inside the doctype.

Fix: `scan_doctype_body` captures a raw source range
(`InputBuffer::raw_range(content_start, pos)`), because every consumed byte
up to the depth-0 `>` is body content. `parse_dtd` and its raw-byte
declaration scanners are unchanged.

## Bonus: pre-existing ASan fixes the EOL corpus work exposed

Adding DTD-heavy, CRLF-flavored seeds to the `parse` fuzz corpus immediately
surfaced three pre-existing defect classes in the DTD declaration path:

1. **`add_element_decl` leaked the incoming content model when a real
   declaration already existed** (upstream `valid.c xmlAddElementDecl`
   frees the incoming content in that branch). Reproducer:
   `<!DOCTYPE a [AYe "v\r\nw">\r\n<!ELEMENT a (#PCDATA)>\r\n<a>&e;\r\ntail</a>`
   — the SAX layer registers the element first; the Rust-side re-parse then
   returned the existing decl and dropped the freshly built `#PCDATA` model.
2. **`collect_attlist_defaults` dropped enumeration chains** built by
   `parse_attr_type` (mirror scan never attaches them).
3. **`add_attribute_decl` leaked the incoming enumeration tree** on the
   duplicate-key path and on hash-add failure (upstream `valid.c
   xmlAddAttributeDecl` frees it in the dup branch).
4. **`process_dtd_fragment` panicked** (`range start index out of range`) on
   a fragment ending in a lone `<` — `<!DOCTYPE a [x<]><a/>` crashed; the
   oracle reports "Content error in the internal subset" (errNo 118) — the
   *diagnostic* side of that input is a separate pre-existing gap (the
   `doctrunc`/INT_SUBSET family, tracked separately).

All four are fixed (free on ownership-transfer-drop paths; bounds guard in
the fragment scanner).

## Verification (all green)

- `cargo test --lib`: **1263 passed / 0 failed** (3 new EOL regression
  tests: `test_eol_normalization_in_text_cdata_comment_pi`,
  `test_attr_value_literal_whitespace_becomes_space`,
  `test_doctype_transport_raw_entity_value_but_expansion_normalized`, plus
  `test_crlf_decode_substitution_forms` in input.rs).
- **Oracle A/B byte-identical** (node-dump probe + `xmllint --format`):
  text/CDATA/comment/PI/attr/doctype CRLF variants incl.
  `ent\r\nval` raw storage + NOENT `ent\nval` expansion, tabs/space runs,
  char-refs, PI trailing data, whitespace-only text runs.
- **PHP six-gate**: 1250 passed / **0 failures** (run twice, incl. after the
  DTD fixes).
- **CLI xmllint differential**: 46/48 byte-identical; the 2 failures
  (CLI-XMLLINT-0019 `--html`, CLI-XMLLINT-0024 `--version`) are
  **pre-existing on clean HEAD** (verified via stash).
- **ASan fuzz**: parse 939k runs, html 1.1M, xpath 1.1M — clean; all five
  pre-existing leak artifacts now pass.
- **lxml differential smoke** (oracle vs candidate containers): identical on
  CRLF/DTD/attr/PI content.

## Noted pre-existing gaps (unchanged, tracked elsewhere)

- `<!DOCTYPE a [x<]><a/>` diagnostic parity (oracle errNo 118 "Content error
  in the internal subset"; candidate parses silently) — the
  `doctrunc`/INT_SUBSET diagnostic family.
- CLI-XMLLINT-0019/0024 failures.
- Undeclared `&e;` in attribute values (oracle: parser error + NULL doc;
  candidate: silent empty value).

## Receipt artifacts

- `courts/suites/phase14/consumers/node-dump-probe.c` — new diagnostic probe
  (raw DOM child dump, serializer-free) used for all node-level A/B work.
- Regression tests in `src/xml/parser/tests.rs` + `src/xml/parser/input.rs`.
