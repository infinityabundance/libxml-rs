#!/usr/bin/env python3
"""pushdiff-decoder-gate.py — the decoder-specific projection of the §16.7.8
push court.

The full push-differential court compares *whole* per-call traces, so it stays
red until the persistent push engine replaces the transitional replay: the
lifecycle divergences (class 1: when `startDocument`/`endDocument` fire,
`ctxt->instate`, `cur-base`, `nameNr`) are independent of source decoding.

This gate isolates the layer that the progressive source decoder owns
(`InputBuffer`'s park/decide/carry machinery) by projecting each trace onto the
decoder-relevant observables only:

  * the ordered SAX events that carry decoded *content* — element names,
    characters payloads, attributes, PIs, comments — with the lifecycle doc
    events (`startDocument`/`endDocument`) removed;
  * whether the encoder error (`dom=8 code=81`) was raised, and with what
    domain/code/level;
  * the `FINAL` (terminating-call) `rc`/`err`/`wf` triple.

Everything else (per-call `in=`, `p=`, `n=`, event timing, and the REFEED
line) belongs to the lifecycle classes and is deliberately NOT compared here.
The REFEED line is divergence class 4 (finished-context refeed, "Extra content
at the end of the document"), which the slice-0 baseline already counts
separately for every document; the gate reports how many cells differ *only*
there, but does not gate on it.

Usage:
  python3 pushdiff-decoder-gate.py <raw-run-dir> [--verbose]

`<raw-run-dir>` is the directory `pushdiff-run.sh` wrote (`oracle-*`/`cand-*`
trace pairs plus `docs.list`). Exit status 0 = decoder gate green.
"""

import glob
import os
import re
import sys

LIFECYCLE = {"startDocument", "endDocument"}
ENC_ERROR_CODE = "81"
FEED_PREFIX = "CALL"

# Decoder-irrelevant call metadata inside a FINAL/REFEED line.
FINAL_FIELDS = ("rc", "err", "wf")


def project(path):
    """Project one probe trace onto the decoder-relevant observables.

    Trace shape (pushdiff-probe.c): a `== ...` header, then per call a
    `> CALL n len=k` feed line, the bare EVENT lines it produced
    (`startDocument`, `startElementNs ...`, `characters ...`, `error dom=...`),
    and a `< CALL n rc=... err=... wf=... in=... p=...` result line; then
    `> FINAL` / `< FINAL ...` and `> REFEED` / `< REFEED ...`.
    """
    events = []
    enc_errors = []
    finals = []
    refeed_lines = []
    pending_text = None
    try:
        handle = open(path, encoding="utf-8", errors="replace")
    except OSError:
        return None
    with handle:
        for raw in handle:
            line = raw.rstrip("\n")
            if not line or line.startswith("=="):
                continue
            if line.startswith(">"):
                continue  # feed markers (`> CALL n len=k`, `> FINAL`, ...)
            if line.startswith("< "):
                body = line[2:].strip()
                if body.startswith("REFEED"):
                    # Divergence class 4 — recorded, never gated (see header).
                    refeed_lines.append(body)
                elif body.startswith("FINAL"):
                    fields = dict(
                        kv.split("=", 1) for kv in body.split() if "=" in kv
                    )
                    finals.append(
                        (body.split()[0],)
                        + tuple(fields.get(f, "?") for f in FINAL_FIELDS)
                    )
                continue
            # A bare line: a SAX event or an error record.
            if line.startswith("error "):
                fields = dict(
                    kv.split("=", 1) for kv in line.split()[1:] if "=" in kv
                )
                if fields.get("code") == ENC_ERROR_CODE:
                    enc_errors.append(
                        (
                            fields.get("dom"),
                            fields.get("code"),
                            fields.get("level"),
                            fields.get("msg"),
                        )
                    )
                # Non-encoding errors are lifecycle/parser-class observations:
                # they still show up as missing/extra content events.
                continue
            if line in LIFECYCLE:
                continue
            # Character-data SEGMENTATION is divergence class 5 (the
            # availability gate / raw-CR behaviour), which is separately
            # documented and still open: collapse consecutive `characters`
            # events into one so the gate compares the decoded TEXT, not the
            # callback boundaries.
            if line.startswith("characters len="):
                if pending_text is not None:
                    pending_text = _merge_characters(pending_text, line)
                else:
                    pending_text = line
                continue
            if pending_text is not None:
                events.append(pending_text)
                pending_text = None
            events.append(line)
        if pending_text is not None:
            events.append(pending_text)
    return events, enc_errors, finals, refeed_lines


def _merge_characters(acc, line):
    """Concatenate two `characters len=N [payload]` records."""

    def parts(rec):
        head, _, tail = rec.partition(" [")
        n = int(head.split("len=")[1])
        payload = tail[:-1] if tail.endswith("]") else tail
        return n, payload

    n1, p1 = parts(acc)
    n2, p2 = parts(line)
    return f"characters len={n1 + n2} [{p1}{p2}]"


def cases(raw_dir):
    """Every `enc-*` (document, mode) cell with both traces present.

    Trace names are `oracle-<corpus-flattened-doc>__<mode>` (the corpus tree is
    flattened into the file name).
    """
    out = []
    for oracle in sorted(glob.glob(os.path.join(raw_dir, "oracle-*"))):
        base = os.path.basename(oracle)[len("oracle-"):]
        if "enc-" not in base:
            continue
        cand = os.path.join(raw_dir, "cand-" + base)
        if os.path.exists(cand):
            out.append((base, oracle, cand))
    return out


def main():
    if len(sys.argv) < 2:
        print(__doc__.strip())
        return 2
    raw_dir = sys.argv[1]
    verbose = "--verbose" in sys.argv

    cells = cases(raw_dir)
    if not cells:
        print(f"pushdiff-decoder-gate: no enc-* trace pairs under {raw_dir}")
        return 2

    failed = []
    class4_only = 0
    for name, oracle_path, cand_path in cells:
        o = project(oracle_path)
        c = project(cand_path)
        reasons = []
        if o is None or c is None:
            reasons.append("missing trace")
        else:
            o_events, o_errs, o_finals, _ = o
            c_events, c_errs, c_finals, _ = c
            if o_events != c_events:
                reasons.append(
                    f"decoded content differs "
                    f"(oracle {len(o_events)} events, candidate {len(c_events)})"
                )
            if o_errs != c_errs:
                reasons.append(f"encoder error differs: {o_errs!r} != {c_errs!r}")
            if o_finals != c_finals:
                reasons.append(f"FINAL differs: {o_finals!r} != {c_finals!r}")
        if reasons:
            failed.append((name, reasons))
            if verbose:
                print(f"FAIL {name}")
                for r in reasons:
                    print(f"       {r}")
        elif verbose:
            print(f"ok   {name}")
        # Informational: cells whose full trace also diverges in the REFEED
        # line (class 4) — expected, never gated here.
        if o is not None and c is not None:
            if o[3] != c[3]:
                class4_only += 1

    total = len(cells)
    print(
        f"pushdiff decoder gate: {total - len(failed)}/{total} cells green "
        f"(decoded content, encoder error, FINAL rc/err/wf)"
    )
    print(
        f"  (informational: {class4_only}/{total} cells also differ in the "
        f"REFEED line — divergence class 4, not gated)"
    )
    if failed:
        print("failing cells:")
        for name, reasons in failed[:20]:
            print(f"  {name}: {'; '.join(reasons)}")
        if len(failed) > 20:
            print(f"  ... and {len(failed) - 20} more")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
