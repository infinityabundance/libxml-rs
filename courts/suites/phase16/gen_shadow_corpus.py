#!/usr/bin/env python3
"""§16.7.8 pushdrive shadow court — adversarial corpus for the persistent driver.

Generates the documents the ORACLE-SHADOW court drives through both providers:

  oracle : libxml2 2.15.3 via xmlCreatePushParserCtxt + xmlParseChunk
           (courts/suites/phase16/pushdiff-probe.c)
  driver : the court-only persistent driver
           (src/xml/parser/pushdrive.rs, driven from pushshadow.rs)

# The two sides must be fed byte-identical documents, so the corpus is generated
# here rather than written twice by hand. Both encoder cells below are GREEN in
# the shadow court as of step 6b; they are kept because they pin two distinct
# contracts (EOF flush vs definite-invalid ingress).

Documents 1-5 are the driver's own court documents: the grammar the driver
covers today. 6-7 are long single constructs (the cases that are quadratic
without lookahead continuation). 8 is the ENCODER-FLUSH case: a UTF-16LE
document whose source stream ends with an incomplete code unit, so
`xmlParserCheckEOF`'s flush reports XML_ERR_INVALID_ENCODING on the terminating
call. 9 is the physical-window threshold archaeology (the `shadow-win-*`
documents). 10 is the DEFINITE-invalid counterpart to 8: it fails on the call
that delivers the bad unit rather than on termination.

Python stdlib only (the court image has no site-packages).
"""
import os
import sys

DOCS = []


def doc(name, data):
    DOCS.append((name, data))


# ── 1-5: the persistent driver's grammar ────────────────────────────────
doc("shadow-minimal.xml", b"<a/>")
doc("shadow-text.xml", b"<a>x</a>")
doc("shadow-nested.xml", b"<a><b/></a>")
doc("shadow-attr.xml", b'<a p="v"/>')
doc("shadow-ns.xml", b'<a xmlns:x="urn:u"><x:b/></a>')

# ── 6-7: one huge construct each ────────────────────────────────────────
LONG = 4096
doc("shadow-longtext.xml", b"<r>" + b"x" * LONG + b"</r>")
doc("shadow-longattr.xml", b'<r a="' + b"x" * LONG + b'"/>')

# ── 8: the encoder-flush cell ─────────────────────────────────────
# A well-formed UTF-16LE document (BOM + 8 code units) followed by ONE stray
# source byte. The document itself parses cleanly to EOF with every
# materialized byte consumed, so nothing is left as "extra content": the only
# remaining defect is the decoder's pending half unit, which upstream's
# xmlParserCheckEOF flush turns into XML_ERR_INVALID_ENCODING on the
# terminating call (green since step 6b).
doc(
    "shadow-utf16trunc.xml",
    b"\xff\xfe" + "<a>x</a>".encode("utf-16-le") + b"\x3c",
)

# ── 9: the physical-window threshold archaeology ───────────────────────
# The shrink gate is `cur - base > 4096` and it keeps LINE_LEN (80) bytes as
# error context, so the interesting variable is the CONSUMED CURSOR when a pass
# begins, not the document size as such. Two shapes, because they cross the
# threshold at different moments:
#
#   text  the run is consumable incrementally, so `used` grows steadily and the
#         first pass whose top exceeds 4096 shrinks mid-document;
#   attr  the tag stays PARKED until its closing `>` arrives, so `used` stays 0
#         and jumps by the whole tag in one step — the shrink then happens on
#         the terminating call.
for n in (4095, 4096, 4097, 8192):
    assert n >= 16
    doc(f"shadow-win-text-{n}.xml", b"<r>" + b"x" * (n - 7) + b"</r>")
    doc(f"shadow-win-attr-{n}.xml", b'<r a="' + b"x" * (n - 9) + b'"/>')

# ── 10: the DEFINITE-invalid encoder cell ──────────────────────────────
# A lone low surrogate is invalid the moment its two bytes are present, so it
# is not a suspension that a terminating call later flushes: upstream's
# xmlParserInputBufferPush fails and xmlParseChunk reports
# XML_ERR_INVALID_ENCODING on THAT call, before xmlParseTryOrFinish runs — so
# none of the call's newly decoded content is parsed and no grammar event may
# fire. Distinct pathway from document 8.
doc(
    "shadow-utf16invalid.xml",
    b"\xff\xfe" + "<a>".encode("utf-16-le") + b"\x00\xdc",
)

# ── 11: the lexical constructs of MISC / PROLOG / EPILOG / CONTENT ──────
# XML declaration, PIs, comments and CDATA. The driver only performs
# upstream's availability gate and then runs the recursive parser's own
# tokenizer scan + recorder, so these cells pin the per-call STATE (instate,
# position, errors) and payload segmentation, not merely that the bytes parse.
#
doc("shadow-decl.xml", b'<?xml version="1.0"?><a>x</a>')
doc(
    "shadow-decl-full.xml",
    b'<?xml version="1.0" encoding="UTF-8" standalone="yes"?><a/>',
)
doc("shadow-pi.xml", b"<?pi before?><a><?pi inside?>x</a>")
doc("shadow-comment.xml", b"<!--top--><a><!--mid-->x</a><!--tail-->")
doc("shadow-cdata.xml", b"<a><![CDATA[x<y&z]]></a>")

# ── 12: adversarial declaration / PI cells ──────────────────────────────
# The declaration is the one construct whose failure must NOT fire
# startDocument (upstream's XML_DECL arm sets `instate = XML_PARSER_MISC`
# unconditionally, but the `while (disableSAX == 0)` loop then exits), and
# `<?xml?>` is a reserved-name PI at document start.
doc("shadow-decl-bad.xml", b'<?xml version="2.0"?><a/>')
doc("shadow-pi-reserved.xml", b"<?xml?><a/>")


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "."
    os.makedirs(out, exist_ok=True)
    for name, data in DOCS:
        with open(os.path.join(out, name), "wb") as f:
            f.write(data)
        print(f"{name} bytes={len(data)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
