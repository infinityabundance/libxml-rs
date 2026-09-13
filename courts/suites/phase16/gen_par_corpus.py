#!/usr/bin/env python3
"""gen_par_corpus.py — §16.8 Rayon parallel-blocking differential corpus.

Generates XML documents whose parse trees exercise the two §16.8 escalation
paths:

  * the per-run probe/prepass crossover (`BLOCK` = 64 KiB, `COARSE` = 4 KiB,
    and the 256 KiB production `threshold`), with comment/CDATA content runs
    landing exactly on, one byte either side of, and spanning those
    boundaries, including terminators planted at boundary +/- 1;
  * the many-medium-run cumulative escalation (dozens of runs each far below
    the crossover);
  * non-ASCII UTF-8 content runs (Level B validator boundary cases).

Deterministic: python's random with a fixed seed (no wall-clock, no
os.urandom), so the same corpus is regenerated for every court run.

Usage: gen_par_corpus.py OUTDIR
"""

import os
import random
import sys

rng = random.Random(0x16_8_0_0_0_0)

BLOCK = 64 * 1024
COARSE = 4096
THRESH = 256 * 1024

# Comment/CDATA-safe content bytes (no '-', ']', CR, LF, or non-ASCII).
CLEAN_C = [c for c in range(0x20, 0x7F) if c not in (ord("<"), ord("&"), ord("]"), ord("-"))]
CLEAN_D = [c for c in range(0x20, 0x7F) if c not in (ord("<"), ord("&"), ord("]"))]

# Valid UTF-8 sequences for text content.
UTF8_CHARS = [
    "é".encode(), "€".encode(), "😀".encode(), "ß".encode(), "Ω".encode(),
]


def clean(n, alphabet):
    return bytes(rng.choice(alphabet) for _ in range(n))


def write(outdir, name, data):
    with open(os.path.join(outdir, name), "wb") as f:
        f.write(data)


def main():
    outdir = sys.argv[1]
    os.makedirs(outdir, exist_ok=True)

    # 1. Single comment runs at every boundary +/- 1.
    lens = set()
    for base in (0, COARSE, BLOCK, THRESH, 2 * THRESH, 4 * THRESH):
        for d in (-2, -1, 0, 1, 2):
            if base + d > 0:
                lens.add(base + d)
    for L in sorted(lens):
        write(outdir, f"c_one_{L:07d}.xml", b"<r><!--" + clean(L, CLEAN_C) + b"--><a/></r>")

    # 2. Single CDATA runs at the same boundaries.
    for L in sorted(lens):
        write(outdir, f"d_one_{L:07d}.xml", b"<r><![CDATA[" + clean(L, CLEAN_D) + b"]]><a/></r>")

    # 3. Many-run documents: runs spanning the cumulative break-even
    #    (base/8) at 64/256/1024 runs and run sizes 4 KiB .. 1 MiB.
    for run_len in (COARSE, 2 * COARSE, BLOCK, 2 * BLOCK, THRESH):
        for runs in (16, 64):
            body = b"".join(b"<!--" + clean(run_len, CLEAN_C) + b"-->" for _ in range(runs))
            write(outdir, f"c_many_{run_len:07d}_{runs:04d}.xml", b"<r>" + body + b"</r>")
            body = b"".join(b"<![CDATA[" + clean(run_len, CLEAN_D) + b"]]>" for _ in range(runs))
            write(outdir, f"d_many_{run_len:07d}_{runs:04d}.xml", b"<r>" + body + b"</r>")

    # 4. Double-hyphen / single-bracket content inside long runs (the per-char
    #    decisions must be preserved across the bulk-skip boundary).
    for L in (COARSE - 1, COARSE, COARSE + 1, BLOCK - 1, BLOCK, BLOCK + 1):
        body = bytearray(clean(L, CLEAN_C))
        body[L // 2] = ord("-")
        write(outdir, f"c_hyphen_{L:07d}.xml", b"<r><!--" + bytes(body) + b"--></r>")
        body = bytearray(clean(L, CLEAN_D))
        body[L // 2] = ord("]")
        write(outdir, f"d_bracket_{L:07d}.xml", b"<r><![CDATA[" + bytes(body) + b"]]></r>")

    # 5. Non-ASCII text runs (valid UTF-8) at boundary lengths.
    for L in (0, 1, 32, 63, 64, 65, 4095, 4096, 4097, 65535, 65536, 65537):
        parts = []
        total = 0
        while total < L:
            ch = rng.choice(UTF8_CHARS)
            if total + len(ch) > L:
                break
            parts.append(ch)
            total += len(ch)
        write(outdir, f"u_text_{L:07d}.xml", b"<r>" + b"".join(parts) + b"</r>")

    # 6. Dense markup with a lone 100 KiB comment: the adaptive probe must not
    #    build a whole-input prepass (this is the measured non-regression case).
    parts = [b"<r>"]
    nbytes = 4
    i = 0
    inserted = False
    target = 4 * 1024 * 1024
    while nbytes < target:
        if not inserted and nbytes >= target // 2:
            c = b"<!--" + clean(100 * 1024, CLEAN_C) + b"-->"
            parts.append(c)
            nbytes += len(c)
            inserted = True
        s = ("<i id=\"i%d\">v%d</i>" % (i, i)).encode()
        parts.append(s)
        nbytes += len(s)
        i += 1
    parts.append(b"</r>")
    write(outdir, "x_dense_onecomment.xml", b"".join(parts))

    # 7. Mixed: comment, CDATA, PI, text, entities, elements at scale.
    parts = [b"<r>"]
    nbytes = 3
    i = 0
    while nbytes < 2 * 1024 * 1024:
        pick = i % 5
        if pick == 0:
            s = b"<!--" + clean(4096 + (i % 3), CLEAN_C) + b"-->"
        elif pick == 1:
            s = b"<![CDATA[" + clean(5000 + (i % 7), CLEAN_D) + b"]]>"
        elif pick == 2:
            s = b"<?pi " + clean(100, CLEAN_C).replace(b"<", b"x") + b"?>"
        elif pick == 3:
            s = b"text" + "é".encode() * 50
        else:
            s = ("<i>%d</i>" % i).encode()
        parts.append(s)
        nbytes += len(s)
        i += 1
    parts.append(b"</r>")
    write(outdir, "x_mixed.xml", b"".join(parts))

    print(f"wrote {len(os.listdir(outdir))} documents")


if __name__ == "__main__":
    main()
