# simplexml S4 — PI data keeps trailing whitespace (GH-12167)

Closed 2026-09-03. Full suite **239 → 238**, zero regressions. simplexml 5 → 4
(gh12167 PASS); dom 152 / xml 0 / xmlreader 29 / xmlwriter 1 (W5) / xsl 52
unchanged. Log: phpbuild-c:/out/pi-six.log (1291-run, 238 failed / 40 skipped).

## Root cause (mirrors parser.c xmlParsePI)

The tokenizer's PI scan (src/xml/parser/tokenizer.rs, scan_pi_or_xml_decl)
skipped only ONE whitespace byte after the target and then TRIMMED the
trailing whitespace from the data ("libxml2 behavior" comment). Upstream
`xmlParsePI` does neither:

- `SKIP_BLANKS` consumes ALL blanks between the target and the data start;
- the content loop copies every character up to the `?` of the `?>` terminator
  — including whitespace immediately before it.

So the PI node content of `<?foo pi contents ?>` is `"pi contents "` (length
12). The candidate stored `"pi contents"` (11), which SimpleXML's string()
exposed (GH-12167) and the xmlreader/dom PI data surfaces share.

## Guard

parser/tests.rs `test_pi_data_keeps_trailing_space_skips_all_leading_blanks`:
`<?foo pi contents ?>` → content `pi contents `; `<?a   b  ?>` → `b  ` (all
leading blanks skipped, trailing kept).

cargo test --lib 1231 pass / 1 ignored; clippy no new warnings; fmt clean.
