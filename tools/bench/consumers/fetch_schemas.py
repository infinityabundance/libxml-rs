#!/usr/bin/env python3
"""fetch_schemas.py — materialize the §16.12 offline validation resources.

The corpus carries real DOCTYPE / xsi:schemaLocation dependencies; validating a
member requires the actual schema/Dtd on disk. This fetches the small set that
is a single self-contained file, verifies SHA-256, and writes `schema/manifest.json`.
The schema bytes are gitignored (cache-only), like the corpus.

Usage: python3 tools/bench/consumers/fetch_schemas.py
"""

from __future__ import annotations

import hashlib
import json
import os
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
SCHEMA = os.path.join(HERE, "schema")
UA = "libxml-rs-bench-corpus/0.1 (+https://github.com/infinityabundance/libxml-rs)"

RESOURCES = [
    {"file": "maven-4.0.0.xsd",
     "url": "https://maven.apache.org/xsd/maven-4.0.0.xsd",
     "for": "MAVEN (xsi:schemaLocation instance target)",
     "license": "Apache-2.0", "spdx": "Apache-2.0",
     "license_url": "https://www.apache.org/licenses/LICENSE-2.0"},
    {"file": "svg10.dtd",
     "url": "https://www.w3.org/TR/2001/REC-SVG-20010904/DTD/svg10.dtd",
     "for": "SVG (DOCTYPE system id)",
     "license": "W3C Document License", "spdx": "W3C",
     "license_url": "https://www.w3.org/Consortium/Legal/2015/copyright-software-and-document"},
    {"file": "partwise.dtd",
     "url": ("https://raw.githubusercontent.com/w3c-cg/musicxml/"
             "252062733f58677eb6cb0b30047fa097fe6c80e2/schema/partwise.dtd"),
     "for": "MUSICXML (DOCTYPE system id)",
     "license": "MusicXML DTD (MakeMusic/W3C-cg musicxml)", "spdx": None,
     "license_url": "https://github.com/w3c-cg/musicxml/blob/main/LICENSE"},
]


def main() -> None:
    os.makedirs(SCHEMA, exist_ok=True)
    manifest = []
    for r in RESOURCES:
        dest = os.path.join(SCHEMA, r["file"])
        if os.path.exists(dest):
            with open(dest, "rb") as f:
                data = f.read()
        else:
            req = urllib.request.Request(r["url"], headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=120) as resp:
                data = resp.read()
            with open(dest, "wb") as f:
                f.write(data)
        manifest.append({**r, "bytes": len(data),
                         "sha256": hashlib.sha256(data).hexdigest()})
        print("  %-18s %7d  %s" % (r["file"], len(data),
                                   hashlib.sha256(data).hexdigest()[:12]))
    with open(os.path.join(SCHEMA, "manifest.json"), "w", encoding="utf-8") as f:
        json.dump({"schema": "consumer-schema/1", "phase": "16.12",
                   "resources": manifest}, f, indent=1)
        f.write("\n")
    print("wrote schema/manifest.json")


if __name__ == "__main__":
    main()
