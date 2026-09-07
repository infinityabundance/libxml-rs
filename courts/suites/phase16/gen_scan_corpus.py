#!/usr/bin/env python3
"""gen_scan_corpus.py — §16.7.7 adversarial parse-level scan corpus (deterministic).

Generates XML documents whose parse trees DEPEND on the text-run scanner:
the printable-ASCII text-run classification must split runs at exactly the
right bytes (<, &, ], CR, LF, control, non-ASCII), so any backend divergence
changes the DOM text segments the node-dump probe prints.

Deterministic: python's random with a fixed seed (no wall-clock, no os.urandom).

Usage: gen_scan_corpus.py OUTDIR
"""

import os
import random
import sys

rng = random.Random(0x16_7_7_7_7)  # §16.7.7

# Content bytes: printable ASCII minus the structural set — the fast-path set.
CONTENT = [c for c in range(0x20, 0x7F) if c not in (ord("<"), ord("&"), ord("]"))]


def text_of_len(n, rng):
    return bytes(rng.choice(CONTENT) for _ in range(n))


def write(outdir, name, data: bytes):
    with open(os.path.join(outdir, name), "wb") as f:
        f.write(data)


def main():
    outdir = sys.argv[1]
    os.makedirs(outdir, exist_ok=True)
    n = 0

    # 1. Pure text runs at every length through the vector-lane boundary
    #    zones and beyond (0..=132 step 1, then wider steps to 4096).
    lengths = list(range(0, 133)) + list(range(140, 600, 10)) + \
        [640, 700, 768, 1000, 2048, 3072, 4096]
    for L in lengths:
        body = text_of_len(L, rng)
        write(outdir, f"t_len_{L:05d}.xml", b"<a>" + body + b"</a>")
        n += 1

    # 2. Structural byte planted at every lane position for lane widths
    #    32 and 64, near both ends of the run (positions 0..70 and L-5..L).
    for L in (40, 96, 160, 300):
        positions = sorted({p for p in list(range(0, 72)) + [L - 5, L - 4, L - 3, L - 2, L - 1]
                            if 0 <= p < L})
        for pos in positions:
            for special in (b"<", b"&", b"]"):
                body = bytearray(text_of_len(L, rng))
                body[pos] = special[0]
                write(outdir, f"t_spec_{L}_{pos}_{special[0]:02x}.xml",
                      b"<a>" + bytes(body) + b"</a>")
                n += 1

    # 3. `]]>` crossing a lane boundary inside text (illegal; error path).
    for pos in (30, 31, 32, 33, 62, 63, 64, 65, 66, 126, 127, 128):
        body = bytearray(text_of_len(pos, rng))
        body.extend(b"]]>rest")
        write(outdir, f"t_cdataclose_{pos}.xml", b"<a>" + bytes(body) + b"</a>")
        n += 1

    # 4. CR/LF inside a run — must end the run for §2.11 handling.
    for pos in (0, 1, 31, 32, 33, 63, 64, 65, 100):
        for eol in (b"\r", b"\n", b"\r\n"):
            body = bytearray(text_of_len(80, rng))
            body[pos:pos] = eol
            write(outdir, f"t_eol_{pos}_{eol[0]:02x}_{len(eol)}.xml",
                  b"<a>" + bytes(body) + b"</a>")
            n += 1

    # 5. UTF-8 high bytes terminating or crossing the run (2/3/4-byte seqs).
    for pos in (0, 31, 32, 33, 63, 64, 65, 95, 96, 97, 127, 128, 129, 200):
        body = bytearray(text_of_len(260, rng))
        for seq in (b"\xc3\xa9", b"\xe2\x82\xac", b"\xf0\x9f\x98\x80"):
            b2 = bytearray(body)
            b2[pos:pos] = seq
            write(outdir, f"t_utf8_{pos}_{seq[0]:02x}.xml", b"<a>" + bytes(b2) + b"</a>")
            n += 1

    # 6. Nested markup: element boundary / entity at the end of a run.
    for L in (31, 32, 33, 63, 64, 65, 127, 128, 129):
        body = text_of_len(L, rng)
        write(outdir, f"t_elemsplit_{L}.xml", b"<a>" + body + b"<b>y</b></a>")
        write(outdir, f"t_entend_{L}.xml",
              b"<a>" + body + b"&amp;</a>")
        n += 2

    # 7. Control / non-ASCII-heavy text (mixed with content bytes).
    write(outdir, "t_mixed_controls.xml",
          b"<a>" + bytes([0x20, 0x7E, 0x1F, 0x00, 0x7F, 0x80, 0xFF]) * 40 + b"</a>")
    n += 1
    write(outdir, "t_long_run_4m.xml",
          b"<a>" + text_of_len(4 * 1024 * 1024, rng) + b"</a>")
    n += 1
    # Markup-heavy: 2 MiB of short runs.
    write(outdir, "t_markup_heavy.xml",
          b"<a>" + b"".join(b"<i>" + text_of_len(rng.randint(1, 40), rng) + b"</i>"
                            for _ in range(20000)) + b"</a>")
    n += 1

    # 8. Structural features that carry text across segments.
    write(outdir, "f_entities.xml",
          b"<!DOCTYPE r [<!ENTITY e 'expanded text &amp; more'>]><r>&e;</r>")
    write(outdir, "f_cdata.xml", b"<r><![CDATA[free text < & ] with runs]]></r>")
    write(outdir, "f_comment.xml", b"<r><!-- comment < & ] --><a>x</a></r>")
    write(outdir, "f_pi.xml", b"<r><?pi data < & ?>x</r>")
    write(outdir, "f_doctype_int.xml",
          b"<!DOCTYPE r [<!ELEMENT r (#PCDATA|b)*><!ELEMENT b EMPTY>]><r>t<b/>u</r>")
    write(outdir, "f_attr_quotes.xml",
          b'<r a="text &lt; &amp; &gt; run" b=\'single \' quote\'><x/></r>')
    n += 6

    # 9. Malformed (error-path): probe must report the SAME failure per
    #    backend. Errors go through the text scanner too.
    write(outdir, "e_trunc_tag.xml", b"<r>abc<")
    write(outdir, "e_trunc_text.xml", b"<r>abc")
    write(outdir, "e_bare_amp.xml", b"<r>a&b</r>")
    write(outdir, "e_cdataclose.xml", b"<r>a]]>b</r>")
    write(outdir, "e_trunc_comment.xml", b"<r><!-- abc")
    write(outdir, "e_trunc_cdata.xml", b"<r><![CDATA[abc")
    write(outdir, "e_doctype_trunc.xml",
          b"<!DOCTYPE r [<!ELEMENT r (a", )
    write(outdir, "e_nul.xml", b"<r>a\x00b</r>")
    write(outdir, "e_high_eof.xml", b"<r>" + b"ab\xc3")
    n += 9

    print(f"generated {n} docs into {outdir}")


if __name__ == "__main__":
    main()
