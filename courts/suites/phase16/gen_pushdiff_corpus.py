#!/usr/bin/env python3
"""gen_pushdiff_corpus.py — adversarial corpus for the §16.7.8 push-parser
differential court.

Each document is small (single-byte chunking must be affordable) and
engineered so that chunk boundaries land inside or exactly at interesting
constructs: text runs, entities, comments, CDATA, PIs, DOCTYPE internal
subsets, attribute values, namespaces, UTF-8 sequences, CR/CRLF line
endings, root/epilog boundaries. The corpus is generated (deterministic —
no timestamps/randomness) into OUT_DIR as *.xml files.

Usage: gen_pushdiff_corpus.py OUT_DIR
"""

import os
import sys


def w(out, name, content):
    # A trailing newline-less doc is interesting too; keep exactly the bytes
    # given. Most docs end with "\n" like real files; some intentionally do
    # not (EOF directly after a construct terminator / inside a construct).
    with open(os.path.join(out, name), "wb") as f:
        f.write(content.encode("utf-8"))


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else "."
    os.makedirs(out, exist_ok=True)
    NL = "\n"
    docs = {
        # --- trivial / boundary shapes -----------------------------------
        "empty.xml": "",
        "blank.xml": "   \n\t\n",
        "minimal.xml": "<a/>",
        "minimal-text.xml": "<a>x</a>",
        "prolog-only.xml": "<?xml version=\"1.0\"?>\n",
        "xml-decl-space.xml": "<?xml version='1.0' encoding='UTF-8' standalone='yes'?>\n<a/>\n",
        "xml-decl-noeol.xml": "<?xml version=\"1.0\"?><a/>",
        "comment-only.xml": "<!--k-->\n",
        "pi-only.xml": "<?pi d?>\n",
        "two-roots.xml": "<a/><b/>",
        "text-then-root.xml": "junk<a/>",
        "root-then-text.xml": "<a/>\njunk",
        "root-then-comment.xml": "<a/>\n<!-- tail -->",
        "root-then-pi.xml": "<a/>\n<?p tail?>",
        # --- text runs -----------------------------------------------------
        "text-long.xml": "<a>" + "".join("line %d with some text\n" % i for i in range(200)) + "</a>",
        "text-no-final-eol.xml": "<a>" + "".join("x" for _ in range(37)) + "</a>",
        "text-crlf.xml": "<a>one\r\ntwo\r\nthree\r\n</a>",
        "text-lone-cr.xml": "<a>one\rtwo\rthree\r</a>",
        "text-cr-end.xml": "<a>text\r",            # doc ENDS in \r (terminate path)
        "text-cr-chunkend.xml": "<a>\r\nmid\r\n</a>\r",  # final byte is \r
        "text-tabs.xml": "<a>\t\ta\t\tb\t</a>",
        "text-unicode.xml": "<a>héllo wörld — ünïcode ✓ 中文 🎉 text</a>",
        "text-unicode-emoji-tail.xml": "<a>abc🎉</a>",
        "text-cjk.xml": "<a>日本語のテキスト</a>",
        "text-nul.xml": "<a>x\u0000y</a>",
        "text-ctrl.xml": "<a>x\u0001y\u000b</a>",
        "text-]]>.xml": "<a>x]]>y</a>",
        "text-]-at-end.xml": "<a>x]</a>",
        "text-]]-at-end.xml": "<a>x]]</a>",
        "text-]]>-split.xml": "<a>x]]><b/>y</a>",
        "text-amp.xml": "<a>&amp;&lt;&gt;&quot;&apos; &#65; &#x42; &#X43; end</a>",
        "text-undef-entity.xml": "<a>x &nope; y</a>",
        "text-entity-in-entity.xml": "<a>&amp;amp;</a>",
        # --- attributes -----------------------------------------------------
        "attr-basic.xml": "<a b=\"1\" c='two' d=\"3\"/>",
        "attr-gt-quoted.xml": "<a b=\"x>y\" c='a>b'/>",
        "attr-lt-in-value.xml": "<a b=\"x<y\"/>",
        "attr-amp.xml": "<a b=\"&amp;&lt;\" c='&#65;'/>",
        "attr-undef.xml": "<a b=\"&nope;\"/>",
        "attr-quotes.xml": "<a b='\"' c=\"'\"/>",
        "attr-]]>.xml": "<a b=\"x]]>y\"/>",
        "attr-crlf.xml": "<a b=\"one\r\ntwo\"/>",
        "attr-nbsp.xml": "<a b=\"x\u00a0y\"/>",
        "attr-dup.xml": "<a b=\"1\" b=\"2\"/>",
        "attr-empty.xml": "<a b=\"\" c=''/>",
        "attr-unicode.xml": "<a b=\"héllo ✓ 中文\"/>",
        # --- constructor-boundary docs (C<plan>: first split lands inside a
        # multibyte sequence / entity ref / attribute value / DTD decl) ----
        "ctor-utf8.xml": "<a>€x</a>",
        "ctor-utf8-cjk.xml": "<a>日本語</a>",
        "ctor-entity.xml": "<a>&amp;y</a>",
        "ctor-charref.xml": "<a>&#x1F600;y</a>",
        "ctor-attr.xml": "<a b=\"v1\">t</a>",
        "ctor-attr-ns.xml": "<r xmlns:z=\"urn:z\" z:k=\"v\"><z:c/></r>",
        "ctor-dtd.xml": "<!DOCTYPE a [<!ELEMENT a EMPTY>]><a/>",
        "attr-ns-decl.xml": "<a xmlns=\"urn:d\" xmlns:x=\"urn:x\" x:b=\"1\"/>",
        "attr-ns-undec.xml": "<a xmlns=\"urn:d\" x:b=\"1\"/>",
        "attr-many.xml": "<a " + " ".join('a%d="v%d"' % (i, i) for i in range(40)) + "/>",
        "attr-nospaces.xml": "<a b='1'c='2'/>",
        "attr-newline-sep.xml": "<a\n b=\"1\"\n c=\"2\"\n/>",
        # --- comments / CDATA / PI in content ------------------------------
        "content-comment.xml": "<a>x<!-- c -->y</a>",
        "content-comment-unterm.xml": "<a>x<!-- c</a>",
        "content-comment-dashdash.xml": "<a><!-- a -- b --></a>",
        "content-cdata.xml": "<a>x<![CDATA[ raw < > & \" ' ]]>y</a>",
        "content-cdata-unterm.xml": "<a><![CDATA[raw</a>",
        "content-cdata-close-in-text.xml": "<a>x]]>y<![CDATA[a]]>b</a>",
        "content-pi.xml": "<a>x<?t d?>y</a>",
        "content-pi-unterm.xml": "<a>x<?t d</a>",
        "content-pi-question.xml": "<a><?t a?b ?></a>",
        # --- elements / structure ------------------------------------------
        "deep.xml": "<a>" * 50 + "x" + "</a>" * 50,
        "deep-selfclose.xml": "<a>" * 30 + "<b/>" + "</a>" * 30,
        "mismatch.xml": "<a><b></a></b>",
        "mismatch-deep.xml": "<a><b><c></b></a>",
        "unclosed-root.xml": "<a><b>x",
        "unclosed-root2.xml": "<a><b>x</b>",
        "empty-elements.xml": "<a><b/><c></c><d/></a>",
        "ws-between.xml": "<a>\n  <b/>\n  <c> \n </c>\n</a>",
        "name-prefix.xml": "<p:a xmlns:p=\"urn:p\"><p:b/><q:c xmlns:q=\"urn:q\"/></p:a>",
        "name-colon.xml": "<a:b:c/>",
        "name-unicode.xml": "<élément attribué=\"1\"/>",
        "name-plus.xml": "<a+b/>",
        "endtag-name.xml": "<a><b></a ></b>",
        "endtag-ws.xml": "<a></a >\n",
        # --- DOCTYPE --------------------------------------------------------
        "doctype-simple.xml": "<!DOCTYPE a>\n<a/>",
        "doctype-extid.xml": "<!DOCTYPE a SYSTEM \"urn:x\">\n<a/>",
        "doctype-pubid.xml": "<!DOCTYPE a PUBLIC \"-//X//EN\" \"urn:x\">\n<a/>",
        "doctype-subset-elem.xml": "<!DOCTYPE a [<!ELEMENT a (#PCDATA)>]>\n<a>x</a>",
        "doctype-subset-choice.xml": "<!DOCTYPE a [<!ELEMENT a (b|c)*>]>\n<a><b/></a>",
        "doctype-subset-seq.xml": "<!DOCTYPE a [<!ELEMENT a (b,c)>]>\n<a><b/><c/></a>",
        "doctype-subset-mixed.xml": "<!DOCTYPE a [<!ELEMENT a (#PCDATA|b)*>]>\n<a>x<b/>y</a>",
        "doctype-subset-empty.xml": "<!DOCTYPE a [<!ELEMENT a EMPTY>]>\n<a/>",
        "doctype-subset-any.xml": "<!DOCTYPE a [<!ELEMENT a ANY>]>\n<a><b/></a>",
        "doctype-entity-decl.xml": "<!DOCTYPE a [<!ENTITY e \"hello\">]>\n<a>&e;</a>",
        "doctype-entity-nested.xml": "<!DOCTYPE a [<!ENTITY e \"<b>x</b>\">]>\n<a>&e;</a>",
        "doctype-entity-param.xml": "<!DOCTYPE a [<!ENTITY % p \"<b/>\">%p;]>\n<a/>",
        "doctype-attr-default.xml":
            "<!DOCTYPE a [<!ATTLIST a t (l|m) \"l\" u CDATA #IMPLIED v ID #REQUIRED>]>\n<a t=\"m\" v=\"i1\"/>",
        "doctype-attr-fixed.xml":
            "<!DOCTYPE a [<!ATTLIST a f CDATA #FIXED \"fx\">]>\n<a/>",
        "doctype-notations.xml":
            "<!DOCTYPE a [<!NOTATION n SYSTEM \"urn:n\"><!ENTITY e SYSTEM \"urn:e\" NDATA n>]>\n<a/>",
        "doctype-unterminated.xml": "<!DOCTYPE a [<!ELEMENT a (#PCDATA)>",
        "doctype-trunc-elem-decl.xml": "<!DOCTYPE a [<!ELEMENT a (#PCDATA",
        "doctype-trunc-choice.xml": "<!DOCTYPE a [<!ELEMENT a (b|c",
        "doctype-trunc-seq.xml": "<!DOCTYPE a [<!ELEMENT a (b,c)>]",
        "doctype-trunc-comment.xml": "<!DOCTYPE a [<!-- x",
        "doctype-trunc-entity.xml": "<!DOCTYPE a [<!ENTITY e \"he",
        "doctype-trunc-cond.xml": "<!DOCTYPE a [<![INCLUDE[<!ELEMENT a EMPTY>",
        "doctype-elem-after-root.xml": "<a/><!DOCTYPE a>",
        "doctype-not-first.xml": "<!--c--><!DOCTYPE a><a/>",
        "doctype-two.xml": "<!DOCTYPE a><!DOCTYPE a><a/>",
        "doctype-in-content.xml": "<a><!DOCTYPE a></a>",
        # --- name/attr edge bytes ------------------------------------------
        "starttag-trunc.xml": "<a b=\"1\"",
        "starttag-trunc-name.xml": "<abc",
        "starttag-trunc-after-name.xml": "<abc ",
        "starttag-trunc-attr-name.xml": "<abc at",
        "starttag-trunc-attr-eq.xml": "<abc at=",
        "starttag-trunc-attr-val-sq.xml": "<abc at='v",
        "starttag-trunc-attr-val-dq.xml": "<abc at=\"v",
        "endtag-trunc.xml": "<a><b></b",
        "endtag-trunc2.xml": "<a></",
        "endtag-trunc3.xml": "<a></a",
        "comment-trunc.xml": "<a><!-- x",
        "comment-trunc2.xml": "<a><!--",
        "pi-trunc.xml": "<a><?t",
        "cdata-trunc.xml": "<a><![CDATA[x",
        "cdata-trunc2.xml": "<a><![C",
        "doctypedecl-trunc.xml": "<!DOCTY",
        "misc-trunc-pi.xml": "<?p ",
        "misc-trunc-comment.xml": "<!-- ",
        "amp-trunc.xml": "<a>&a</a>",
        "amp-trunc2.xml": "<a>&amp</a>",
        "amp-bare.xml": "<a>& </a>",
        "gt-in-text.xml": "<a>x>y</a>",
        "lt-slash-text.xml": "<a>x</ y</a>",
        # --- namespace / misc ----------------------------------------------
        "ns-rescope.xml": "<a xmlns=\"u1\"><b xmlns=\"u2\"><c/></b><d/></a>",
        "ns-default-undeclare.xml": "<a xmlns=\"u1\" xmlns=\"\"><b/></a>",
        "ns-xml-prefix.xml": "<a xml:lang=\"en\"/>",
        "ns-xmlns-prefix.xml": "<a xmlns:xml=\"http://www.w3.org/XML/1998/namespace\"/>",
        "pi-target-xml.xml": "<?xml-stylesheet href=\"x\"?>\n<a/>",
        "bom.xml": "\ufeff<?xml version=\"1.0\"?>\n<a/>",
        "standalone-no.xml": "<?xml version=\"1.0\" standalone=\"no\"?>\n<!DOCTYPE a>\n<a/>",
        # CR at interesting offsets for fixed-chunk modes (end_in_lf): the
        # byte \r appears at position multiples of several small chunk sizes.
        "cr-multi.xml": "<a>\r\n" + "\r\n".join("<b k=\"%d\"/>" % i for i in range(20)) + "\r\n</a>\r\n",
        "cr-solo-multi.xml": "<a>" + "x\ry\rz\r" * 10 + "</a>",
        # Slice-1 CR-chain regressions: CONSECUTIVE raw CRs must not
        # collapse (a withheld-CR flag would lose a byte). These exercise
        # CR->CR->data, CR->CR->CR->data, CR->CR->final and CR->CR->tags
        # under the b1/zK/rN plans.
        "cr-cr.xml": "<a>x\r\ry</a>",
        "cr-cr-cr.xml": "<a>x\r\r\ry</a>",
        "cr-cr-final.xml": "<a>x\r\r",
        "cr-cr-tags.xml": "<a>x\r\r<b/>y</a>",
        "cr-crlf-mix.xml": "<a>x\r\r\n\r\ny</a>",
    }
    for name, content in sorted(docs.items()):
        w(out, name, content)
    # ── Progressive-DECODING corpus (slice 1 blocker) ───────────────────
    # BOMs, encoding signatures and multi-byte code units split across
    # chunk boundaries by the b1/b2/b3 plans. Upstream defers encoding
    # detection in XML_PARSER_START until enough bytes are available (and
    # parks on a non-final call with <4 bytes), and keeps the source decoder
    # installed so later chunks continue decoding.
    utf16le = lambda s: s.encode("utf-16-le")
    utf16be = lambda s: s.encode("utf-16-be")
    utf32le = lambda s: s.encode("utf-32-le")
    utf32be = lambda s: s.encode("utf-32-be")
    enc_docs = {
        "enc-utf16le-bom.xml": b"\xff\xfe" + utf16le("<a>x</a>"),
        "enc-utf16be-bom.xml": b"\xfe\xff" + utf16be("<a>x</a>"),
        "enc-utf16le-nobom.xml": utf16le("<?xml version=\"1.0\"?><a>x</a>"),
        "enc-utf16be-nobom.xml": utf16be("<?xml version=\"1.0\"?><a>x</a>"),
        "enc-utf32le-bom.xml": b"\xff\xfe\x00\x00" + utf32le("<a>x</a>"),
        "enc-utf32be-bom.xml": b"\x00\x00\xfe\xff" + utf32be("<a>x</a>"),
        "enc-utf32be-nobom.xml": utf32be("<?xml version=\"1.0\"?><a>x</a>"),
        "enc-ebcdic.xml": "<?xml version=\"1.0\"?><a>x</a>".encode("cp037"),
        # A surrogate pair (U+1F389) split between calls: 3D D8 | 89 DF.
        "enc-utf16le-surrogate.xml": b"\xff\xfe" + utf16le("<a>\U0001F389</a>"),
        "enc-utf16be-surrogate.xml": b"\xfe\xff" + utf16be("<a>\U0001F389</a>"),
        # Document ENDS with half a UTF-16 code unit (terminating call must
        # report it, not suspend).
        "enc-utf16le-half-end.xml": b"\xff\xfe" + utf16le("<a>x") + b"\x3c",
        "enc-utf16be-half-end.xml": b"\xfe\xff" + utf16be("<a>x") + b"\x00",
        # Streams whose ONLY content is the start of a BOM/signature: the
        # b1/b2 plans cut inside it.
        "enc-bom-le-only.xml": b"\xff\xfe",
        "enc-bom-be-only.xml": b"\xfe\xff",
        "enc-bom-le-half.xml": b"\xff\xfe<\x00",
    }
    for name, content in sorted(enc_docs.items()):
        with open(os.path.join(out, name), "wb") as f:
            f.write(content)
    # A couple of binary-edge files (invalid UTF-8) written as raw bytes.
    raw = {
        "raw-invalid-utf8.xml": b"<a>\xff\xfe</a>",
        "raw-trunc-utf8.xml": b"<a>\xc3</a>",      # 0xC3 then '<': fatal invalid UTF-8
        "raw-trunc-utf8-2.xml": b"<a>\xe2\x82</a>",  # 0xE2 0x82 then '<': fatal
        "raw-overlong.xml": b"<a>\xc0\xaf</a>",
        "raw-high-name.xml": b"<\xf0\xa0\x80</a>",
        # Genuinely EOF-truncated UTF-8: the stream ENDS inside a multibyte
        # sequence — each example below lacks exactly ONE final continuation
        # byte, so the non-final feed must SUSPEND, not error:
        "raw-pending-utf8-2.xml": b"<a>\xc3",
        "raw-pending-utf8-3.xml": b"<a>\xe2\x82",
        "raw-pending-utf8-4.xml": b"<a>\xf0\x9f\x8e",
        # Truncated mid entity-reference name (no ';' yet):
        "raw-pending-entity.xml": b"<a>&am",
    }
    for name, content in sorted(raw.items()):
        with open(os.path.join(out, name), "wb") as f:
            f.write(content)
    print("wrote %d generated docs to %s" % (len(docs) + len(raw) + len(enc_docs), out))


if __name__ == "__main__":
    main()
