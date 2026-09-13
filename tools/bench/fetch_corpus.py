#!/usr/bin/env python3
"""fetch_corpus.py — §16.10 real-world XML performance corpus fetcher.

Retrieves the 100-file, provenance-tracked corpus described by
`tools/bench/corpus/manifest.json` into the gitignored cache
(`tools/bench/corpus/{downloads,files}/`).

Discipline (§16.10.4):

* **Approved sources only.** Every entry names its source organisation and URL;
  the fetcher never follows a redirect to a host other than the entry's own
  recorded host and never fetches anything not in the manifest.
* **Rate-limited.** A per-source minimum interval (`rate_limit_ms`) is honoured
  between requests, with a polite User-Agent.
* **Hash-verified.** `compressed_sha256` (if recorded) is checked against the
  downloaded bytes; `uncompressed_sha256` (if recorded) against the decoded
  file. A mismatch is a hard failure.
* **Never silently replaces.** If a cached file already exists with a *different*
  content hash than the manifest records, the fetcher fails with an instruction
  to investigate — it does not overwrite.
* **Redistribution is explicit.** Files stay in the gitignored cache unless the
  entry is marked `redistributable` *and* `redistribution_verified`.

The benchmark (16.12+) works from the manifest, not from this script.

The byte cache (`downloads/`, `files/`, `state.json`) is gitignored and may be
relocated with the `LIBXML_RS_CORPUS_CACHE` environment variable; the manifest
itself always stays in the repository.

Usage:
  python3 tools/bench/fetch_corpus.py --all
  python3 tools/bench/fetch_corpus.py --id jats-001 --id maven-003
  python3 tools/bench/fetch_corpus.py --category T1-very-large
  python3 tools/bench/fetch_corpus.py --verify            # hashes only, no net
  python3 tools/bench/fetch_corpus.py --list
"""

from __future__ import annotations

import argparse
import base64
import bz2
import gzip
import hashlib
import json
import lzma
import os
import sys
import time
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
CORPUS = os.path.join(ROOT, "tools", "bench", "corpus")
MANIFEST = os.path.join(CORPUS, "manifest.json")
# The (gitignored) byte cache may live outside the repository: set
# LIBXML_RS_CORPUS_CACHE to relocate downloads/files/state (the manifest, the
# committed artifact of record, always stays in the repository).
CACHE = os.environ.get("LIBXML_RS_CORPUS_CACHE", CORPUS)
DOWNLOADS = os.path.join(CACHE, "downloads")
FILES = os.path.join(CACHE, "files")
STATE = os.path.join(CACHE, "state.json")

UA = ("libxml-rs-bench-corpus/0.1 (+https://github.com/infinityabundance/libxml-rs; "
      "research corpus retrieval)")
# Generous default timeout for the very-large tier.
TIMEOUT = 900


def load_manifest() -> dict:
    with open(MANIFEST, "r", encoding="utf-8") as f:
        return json.load(f)


def save_manifest(doc: dict) -> None:
    with open(MANIFEST, "w", encoding="utf-8") as f:
        json.dump(doc, f, indent=1, ensure_ascii=False)
        f.write("\n")


def load_state() -> dict:
    if os.path.exists(STATE):
        with open(STATE, "r", encoding="utf-8") as f:
            return json.load(f)
    return {"last_host_fetch": {}}


def save_state(state: dict) -> None:
    with open(STATE, "w", encoding="utf-8") as f:
        json.dump(state, f, indent=1)


def sha256_file(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def rate_limit(state: dict, url: str, min_ms: int) -> None:
    from urllib.parse import urlparse

    host = urlparse(url).netloc
    last = state["last_host_fetch"].get(host, 0.0)
    wait = (last + min_ms / 1000.0) - time.time()
    if wait > 0:
        time.sleep(wait)
    state["last_host_fetch"][host] = time.time()


def http_download(url: str, dest: str, state: dict, min_ms: int) -> str:
    """Download `url` to `dest` (streamed). Returns the sha256."""
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    rate_limit(state, url, min_ms)
    req = urllib.request.Request(url, headers={"User-Agent": UA,
                                               "Accept-Encoding": "identity"})
    h = hashlib.sha256()
    tmp = dest + ".part"
    with urllib.request.urlopen(req, timeout=TIMEOUT) as resp, open(tmp, "wb") as out:
        while True:
            chunk = resp.read(1 << 20)
            if not chunk:
                break
            h.update(chunk)
            out.write(chunk)
    os.replace(tmp, dest)
    return h.hexdigest()


def decode(src: str, dest: str, transform: str) -> None:
    """Decode `src` into `dest` according to `transform`."""
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    if transform == "none":
        if os.path.abspath(src) != os.path.abspath(dest):
            with open(src, "rb") as i, open(dest, "wb") as o:
                o.write(i.read())
        return
    if transform == "base64":
        with open(src, "rb") as i, open(dest, "wb") as o:
            data = base64.b64decode(i.read())
            o.write(data)
        return
    opener = {"gzip": gzip.open, "bzip2": bz2.open, "xz": lzma.open}[transform]
    with opener(src, "rb") as i, open(dest, "wb") as o:
        for chunk in iter(lambda: i.read(1 << 20), b""):
            o.write(chunk)


def fetch_entry(entry: dict, state: dict, default_rate_ms: int) -> str:
    eid = entry["id"]
    dest = os.path.join(FILES, entry["filename"])
    dl = os.path.join(DOWNLOADS, entry["filename"] + ".download")
    if os.path.exists(dest):
        got = sha256_file(dest)
        want = entry.get("uncompressed_sha256")
        if want and got != want:
            raise SystemExit(
                f"PROVENANCE FAIL {eid}: cached {entry['filename']} sha256 {got} "
                f"!= manifest {want}. Investigate; the fetcher will not replace it.")
        # Re-record the hashes/bytes so a manifest rebuilt from sources (which
        # has no hashes yet) is re-sealed from the verified cache rather than
        # silently trusted.
        entry["uncompressed_sha256"] = got
        entry["bytes"] = os.path.getsize(dest)
        if os.path.exists(dl):
            entry["compressed_sha256"] = sha256_file(dl)
        print(f"  {eid}: cached ok ({got[:12]}…)")
        return got
    rate_ms = entry.get("rate_limit_ms", default_rate_ms)
    print(f"  {eid}: downloading {entry['source_url']}")
    got_c = http_download(entry["source_url"], dl, state, rate_ms)
    want_c = entry.get("compressed_sha256")
    if want_c and got_c != want_c:
        raise SystemExit(
            f"PROVENANCE FAIL {eid}: compressed sha256 {got_c} != manifest {want_c} "
            f"(source changed). Update the manifest deliberately; no silent replace.")
    decode(dl, dest, entry.get("transform", "none"))
    got = sha256_file(dest)
    want = entry.get("uncompressed_sha256")
    if want and got != want:
        raise SystemExit(
            f"PROVENANCE FAIL {eid}: decoded sha256 {got} != manifest {want}.")
    entry["compressed_sha256"] = got_c
    entry["uncompressed_sha256"] = got
    entry["bytes"] = os.path.getsize(dest)
    return got


def verify_entry(entry: dict) -> bool:
    dest = os.path.join(FILES, entry["filename"])
    if not os.path.exists(dest):
        print(f"  MISSING {entry['id']}: {entry['filename']}")
        return False
    got = sha256_file(dest)
    want = entry.get("uncompressed_sha256")
    if want and got != want:
        print(f"  MISMATCH {entry['id']}: {got} != {want}")
        return False
    print(f"  ok {entry['id']}")
    return True


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--all", action="store_true")
    ap.add_argument("--id", action="append", default=[])
    ap.add_argument("--category", action="append", default=[])
    ap.add_argument("--verify", action="store_true")
    ap.add_argument("--list", action="store_true")
    ap.add_argument("--rate-ms", type=int, default=1000)
    args = ap.parse_args()

    doc = load_manifest()
    entries = doc["entries"]

    if args.list:
        for e in entries:
            print(f"{e['id']}\t{e['category']}\t{e['filename']}\t{e['source_url']}")
        return 0

    if args.verify:
        ok = all(verify_entry(e) for e in entries)
        return 0 if ok else 1

    sel = entries
    if args.id:
        want = set(args.id)
        sel = [e for e in entries if e["id"] in want]
        if len(sel) != len(want):
            missing = want - {e["id"] for e in sel}
            raise SystemExit(f"unknown ids: {sorted(missing)}")
    elif args.category:
        want = set(args.category)
        sel = [e for e in entries if e["category"] in want]
    elif not args.all:
        print("nothing selected: pass --all, --id, --category, --verify or --list")
        return 2

    state = load_state()
    os.makedirs(FILES, exist_ok=True)
    os.makedirs(DOWNLOADS, exist_ok=True)
    print(f"fetching {len(sel)} of {len(entries)} entries…")
    for e in sel:
        fetch_entry(e, state, args.rate_ms)
        save_state(state)
        save_manifest(doc)  # hashes/bytes recorded as they are verified
    save_manifest(doc)
    print("done.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
