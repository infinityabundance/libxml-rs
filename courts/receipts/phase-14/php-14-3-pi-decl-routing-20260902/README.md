# KEY-3 — PI-vs-XML-decl routing + reserved-name / not-finished error codes

Closed 2026-09-02. Full suite **276 -> 275**: ext/xml **1 -> 0** — the whole
SP-14.3.1 subphase is now closed (xml_error_string_basic_libxml PASS). Zero
regressions: dom 166 / simplexml 9 / xmlreader 29 / xmlwriter 19 / xsl 52
unchanged.

## Root cause

The tokenizer routed `<?xml` to the XML-declaration scanner for ANY
case-insensitive `<?xml` at byte offset 0. Upstream `xmlParseDocument` only
treats `<?xml` as a declaration when it sits at the LOGICAL document start AND
the byte after "xml" is a blank (CMP5 + `IS_BLANK(NXT(5))`, lowercase).
Everything else (`<?xml?`, `<?xml>`, `<?XML?`, leading-space `<?xml?`,
`<?xml-stylesheet…`) is an ordinary PI whose target must satisfy
`xmlParsePITarget`:

- exact lowercase target "xml" or a 3-char case variant ("XML"/"xMl"/…) →
  FATAL `XML_ERR_RESERVED_XML_NAME` (64);
- xml-prefixed longer targets → warning, except the W3C PIs `xml-stylesheet`
  and `xml-model`;
- a PI never closed by `?>` → FATAL `XML_ERR_PI_NOT_FINISHED` (47), raised
  AFTER the reserved-name error so the final `errNo` is 47 (upstream raises
  both; the later fatal overwrites `errNo`).

The candidate's old routing produced codes 4 ("Not well-formed (invalid
token)") for `<?xml?>` and 57 for `<?xml>`/`<?XML?>` — the exact rows
`xml_error_string_basic_libxml` asserts (47 / 64) — and even broke the legal
`<?xml-stylesheet type="text/xsl" href="…"?>` PI (parsed as a declaration → 57).

## Fix

- tokenizer `scan_pi_or_xml_decl`: declaration route requires the base input
  (`at_base_input()`), the logical document start (`start_pos ==
  doc_start_offset()` — 3 when a UTF-8 BOM was retained, so `bug35447`'s
  BOM+decl stays a declaration), lowercase `xml`, and a blank after it.
- Otherwise the regular-PI path now mirrors `xmlParsePITarget` (reserved
  exact/case-variant names → fatal 64 with the upstream message; W3C list
  exempt; other xml-prefixed → warning 64) and records
  `XML_ERR_PI_NOT_FINISHED` (47) when `?>` never arrives.
- PI tokens carry `unterminated`; the four PI-consuming sites in state.rs
  (parse_prolog, parse_element content, parse_misc_after_root, parse_epilog)
  pause (`truncated_abort`) in silent probes / eager-partial delivery instead
  of firing the partial PI or delivering the error prematurely.
- InputStack: `doc_start_offset()` / `at_base_input()`; InputBuffer:
  `bom_bytes_consumed()`.

## Oracle-pinned (php probes, candidate == oracle on every row)

| input | code | string |
|---|---|---|
| `<?xml?>` | 64 | Reserved XML Name |
| `<?xml>` | 47 | Processing Instruction not finished |
| ` <?xml?>` (leading space) | 64 | Reserved XML Name |
| `<?XML?>` | 64 | Reserved XML Name |
| `<?xml version="dummy">` | 57 | XML declaration not finished |
| `<?xml-stylesheet type="text/xsl" href="x"?><r/>` | ok | — |
| BOM + `<?xml version="1.0"?><r/>` | ok | — |
| `<?xml version="1.0"?><element>` | 77 | Tag not finished |
| `<?xml version="1.0"?><elem></element>` | 76 | Mismatched tag |

Probes kept: consumers/{errstr5-probe.php, pi-probe.php, bomws-probe.php}.

## Guards

- tests.rs `test_pi_vs_xml_decl_routing_error_codes` (64 / 47 / 57 rows +
  BOM+decl + xml-stylesheet control).
- cargo test --lib 1223 pass / 1 ignored; clippy no new warnings (4
  pre-existing); fmt clean.
- log: /out/k3-six-full.log (275), /out/k3-xml.log (ext/xml 0).
