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
without lookahead continuation). 8 is the ENCODER-FLUSH case the driver
documents as unimplemented: a UTF-16LE document whose source stream ends with
an incomplete code unit, so `xmlParserCheckEOF`'s flush must report
XML_ERR_INVALID_ENCODING on the terminating call. It is expected to be the
first RED cell of this court.

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

# ── 9: the DEFINITE-invalid encoder cell ──────────────────────────────────
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
