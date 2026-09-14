#!/usr/bin/env python3
"""build_manifest.py — assemble `tools/bench/corpus/manifest.json` (§16.10.4).

This is the curation driver of record for the 100-file real-world XML
performance corpus. It encodes the approved sources, pinned revisions and
per-file provenance, then (for the two categories whose licence varies per
file — PMC JATS and Maven POM) retrieves the file and extracts the applicable
licence so the manifest records it exactly.

Run:  python3 tools/bench/corpus/build_manifest.py [--check]
      (--check also HEAD-verifies every URL below the large-tier threshold)

The manifest it emits is the canonical artifact; this script is committed so
the curation is auditable and reproducible.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

CORPUS = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(CORPUS))
MANIFEST = os.path.join(CORPUS, "manifest.json")
UA = ("libxml-rs-bench-corpus/0.1 (+https://github.com/infinityabundance/libxml-rs; "
      "research corpus retrieval)")
RETRIEVED = "2026-09-13"


class _SameHostRedirect(urllib.request.HTTPRedirectHandler):
    """Refuse a redirect whose target hostname differs from the request."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):
        old = urllib.parse.urlsplit(req.full_url)
        new = urllib.parse.urlsplit(newurl)
        if (new.hostname or "").lower() != (old.hostname or "").lower():
            raise urllib.error.HTTPError(
                newurl, code,
                "cross-host redirect refused: %s -> %s" % (old.netloc, new.netloc),
                headers, fp)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


OPENER = urllib.request.build_opener(_SameHostRedirect)

# Pinned source revisions (immutable tags/commits).
REV = {
    "tei_stylesheets": "TEIC/Stylesheets@cda7f87ead3d52629556b1d24726ffc1d042be61",
    "tei": "TEIC/TEI@884f8dd3c453595742ed706e1ac155db46731338",
    "music21": "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d",
    "partitura": "CPJKU/partitura@427ff875bd5a49a0eec894fdd7c6631ed7f597ea",
    "xsltng": "docbook/xslTNG@a840909a8c82d23458ba72e61e0eed4185be6b74",
    "xslt10": "docbook/xslt10-stylesheets@efd62655c11cc8773708df7a843613fa1e932bf8",
    "resvg": "linebender/resvg@2f1aa34fb08812c1040e11a0effaedc645a25bd7",
    "batik": "apache/xmlgraphics-batik@a76c1c1113b248a1ddc6304f37f29cde721da9be",
    "gdal": "OSGeo/gdal@587b92c569834283928091bd1e55b32c4ff5de43",
    "libkml": "libkml/libkml@916a801ed3143ab82c07ec108bad271aa441da16",
    "gpsbabel": "GPSBabel/gpsbabel@413be6d8c6a0404ae5ff63f7d4ee78ab003ff8a3",
    "feedvalidator": "w3c/feedvalidator@9ce274c9db93796b8ab2a44952b9da80811bf765",
    "feedparser": "kurtmckee/feedparser@a22c5521cbb109871f1a2318948581901bd47e26",
    "springws": "spring-projects/spring-ws@48fbfccad7ad008d402d0d04e0714f91b685a87b",
    "cxf": "apache/cxf@34917285126fb31e63c14bcdbd297919023748a5",
    "xsdtests": "w3c/xsdtests@7bc3365c652a322f3d762021b3879eb92dae7e30",
    "edgartools": "dgunning/edgartools@abe44344c56cf4bfb5443e0debca7e39342f6e7a",
    "aosp": "platform/...@android-14.0.0_r1 (AOSP release tag)",
}

# Institutional source-org labels.
ORG = {
    "tei_stylesheets": "TEI Consortium (TEIC/Stylesheets)",
    "tei": "TEI Consortium (TEIC/TEI)",
    "music21": "cuthbertLab (music21)",
    "partitura": "JKU Linz (CPJKU/partitura)",
    "xsltng": "DocBook Project (docbook/xslTNG)",
    "xslt10": "DocBook Project (docbook/xslt10-stylesheets)",
    "resvg": "Linebender (resvg)",
    "batik": "Apache Software Foundation (Apache Batik)",
    "gdal": "OSGeo (GDAL)",
    "libkml": "libkml project",
    "gpsbabel": "GPSBabel project",
    "feedvalidator": "W3C (feedvalidator)",
    "feedparser": "kurtmckee (feedparser)",
    "springws": "VMware/Spring (spring-ws)",
    "cxf": "Apache Software Foundation (Apache CXF)",
    "xsdtests": "W3C XML Schema Test Suite",
    "edgartools": "dgunning/edgartools (mirror of U.S. SEC EDGAR filings)",
    "aosp": "Android Open Source Project (AOSP)",
    "europepmc": "Europe PMC / PubMed Central Open Access subset",
    "maven": "Maven Central (repo1.maven.org)",
    "osm": "OpenStreetMap contributors (BBBike city extract)",
    "pubmed": "U.S. National Library of Medicine (PubMed baseline)",
}

LICENSE = {
    "tei_stylesheets": ("BSD-2-Clause", "BSD-2-Clause", "https://github.com/TEIC/Stylesheets/blob/cda7f87ead3d52629556b1d24726ffc1d042be61/COPYING"),
    "tei": ("BSD-2-Clause", "BSD-2-Clause", "https://github.com/TEIC/TEI/blob/884f8dd3c453595742ed706e1ac155db46731338/COPYING"),
    "music21": ("BSD-3-Clause", "BSD-3-Clause", "https://github.com/cuthbertLab/music21/blob/ac6cf27258cbbda118f55470f41adb7a84a6f41d/LICENSE"),
    "partitura": ("Apache-2.0", "Apache-2.0", "https://github.com/CPJKU/partitura/blob/427ff875bd5a49a0eec894fdd7c6631ed7f597ea/LICENSE"),
    "xsltng": ("MIT", "MIT", "https://github.com/docbook/xslTNG/blob/a840909a8c82d23458ba72e61e0eed4185be6b74/LICENSE"),
    "xslt10": ("DocBook XSL custom (MIT-style)", "LicenseRef-DocBook-XSL-Stylesheets", "https://github.com/docbook/xslt10-stylesheets/blob/efd62655c11cc8773708df7a843613fa1e932bf8/COPYING"),
    "resvg": ("Apache-2.0", "Apache-2.0", "https://github.com/linebender/resvg/blob/2f1aa34fb08812c1040e11a0effaedc645a25bd7/LICENSE.txt"),
    "batik": ("Apache-2.0", "Apache-2.0", "https://github.com/apache/xmlgraphics-batik/blob/a76c1c1113b248a1ddc6304f37f29cde721da9be/LICENSE"),
    "gdal": ("MIT", "MIT", "https://github.com/OSGeo/gdal/blob/587b92c569834283928091bd1e55b32c4ff5de43/LICENSE.TXT"),
    "libkml": ("BSD-3-Clause", "BSD-3-Clause", "https://github.com/libkml/libkml/blob/916a801ed3143ab82c07ec108bad271aa441da16/COPYING"),
    "gpsbabel": ("GPL-2.0-or-later", "GPL-2.0-or-later", "https://github.com/GPSBabel/gpsbabel/blob/413be6d8c6a0404ae5ff63f7d4ee78ab003ff8a3/COPYING"),
    "feedvalidator": ("MIT", "MIT", "https://github.com/w3c/feedvalidator/blob/9ce274c9db93796b8ab2a44952b9da80811bf765/LICENSE"),
    "feedparser": ("BSD-2-Clause", "BSD-2-Clause", "https://github.com/kurtmckee/feedparser/blob/a22c5521cbb109871f1a2318948581901bd47e26/LICENSE"),
    "springws": ("Apache-2.0", "Apache-2.0", "https://github.com/spring-projects/spring-ws/blob/48fbfccad7ad008d402d0d04e0714f91b685a87b/LICENSE.txt"),
    "cxf": ("Apache-2.0", "Apache-2.0", "https://github.com/apache/cxf/blob/34917285126fb31e63c14bcdbd297919023748a5/LICENSE"),
    "xsdtests": ("W3C Document License", "W3C", "https://www.w3.org/Consortium/Legal/2015/copyright-software-and-document"),
    "edgartools": ("MIT (repository); EDGAR filings are U.S. Government works", "MIT", "https://github.com/dgunning/edgartools/blob/abe44344c56cf4bfb5443e0debca7e39342f6e7a/LICENSE"),
    "aosp": ("Apache-2.0", "Apache-2.0", "https://source.android.com/docs/setup/about/licenses"),
    "osm": ("ODbL-1.0", "ODbL-1.0", "https://opendatacommons.org/licenses/odbl/1-0/"),
    "pubmed": ("NLM data (U.S. public domain)", "LicenseRef-NLM-PubMed", "https://www.nlm.nih.gov/databases/download/terms_and_conditions.html"),
    "europepmc": ("per-article", None, None),
    "maven": ("per-artifact", None, None),
}

# Required attribution per source (spec §16.10.4).
ATTR = {
    "tei_stylesheets": "Copyright TEI Consortium; BSD-2-Clause (see licence URL).",
    "tei": "Copyright TEI Consortium; BSD-2-Clause (see licence URL).",
    "music21": "Copyright Michael Scott Asato Cuthbert and music21 contributors; BSD-3-Clause.",
    "partitura": "Copyright JKU Linz partitura contributors; Apache-2.0.",
    "xsltng": "Copyright Norman Walsh; MIT.",
    "xslt10": "Copyright Norman Walsh / DocBook Project; DocBook XSL licence.",
    "resvg": "Copyright Linebender contributors; Apache-2.0.",
    "batik": "Copyright The Apache Software Foundation; Apache-2.0.",
    "gdal": "Copyright GDAL contributors; MIT.",
    "libkml": "Copyright Google Inc. and libkml contributors; BSD-3-Clause.",
    "gpsbabel": "Copyright GPSBabel contributors; GPL-2.0-or-later.",
    "feedvalidator": "Copyright W3C; MIT.",
    "feedparser": "Copyright Kurt McKee and feedparser contributors; BSD-2-Clause.",
    "springws": "Copyright VMware/Spring; Apache-2.0.",
    "cxf": "Copyright The Apache Software Foundation; Apache-2.0.",
    "xsdtests": "Copyright W3C; W3C Document License.",
    "edgartools": "U.S. Securities and Exchange Commission public filing content.",
    "aosp": "Copyright The Android Open Source Project; Apache-2.0.",
    "europepmc": "Article authors and journal; retrieved via Europe PMC REST.",
    "maven": "Copyright the respective project authors; per-artifact licence.",
    "osm": "\u00a9 OpenStreetMap contributors, ODbL 1.0 (BBBike extract).",
    "pubmed": "U.S. National Library of Medicine (NLM).",
}

RAW = "https://raw.githubusercontent.com/{repo}/{sha}/{path}"


def raw(key: str, path: str) -> str:
    repo, sha = REV[key].split("@")
    return RAW.format(repo=repo, sha=sha, path=urllib.parse.quote(path, safe="/()'+"))


def entry(eid, filename, category, src, url, transform="none", rate_ms=1000,
          notes=None, eligibility=None):
    lic, spdx, licurl = LICENSE[src]
    return {
        "id": eid,
        "filename": filename,
        "category": category,
        "source_org": ORG[src],
        "source_ref": REV.get(src, src),
        "source_url": url,
        "retrieval_method": "https-get",
        "retrieved_at": RETRIEVED,
        "license": lic,
        "spdx": spdx,
        "license_url": licurl,
        "attribution": ATTR.get(src),
        "transform": transform,
        "rate_limit_ms": rate_ms,
        "redistributable": False,
        "redistribution_verified": False,
        "consumer_eligibility": eligibility or ["perf-bench"],
        "notes": notes,
    }



MAVEN = [
    ("org.apache.commons:commons-lang3:3.14.0",
     "https://repo1.maven.org/maven2/org/apache/commons/commons-lang3/3.14.0/commons-lang3-3.14.0.pom"),
    ("org.apache.commons:commons-text:1.11.0",
     "https://repo1.maven.org/maven2/org/apache/commons/commons-text/1.11.0/commons-text-1.11.0.pom"),
    ("com.google.guava:guava:33.0.0-jre",
     "https://repo1.maven.org/maven2/com/google/guava/guava/33.0.0-jre/guava-33.0.0-jre.pom"),
    ("org.springframework:spring-core:6.1.3",
     "https://repo1.maven.org/maven2/org/springframework/spring-core/6.1.3/spring-core-6.1.3.pom"),
    ("com.fasterxml.jackson.core:jackson-databind:2.16.1",
     "https://repo1.maven.org/maven2/com/fasterxml/jackson/core/jackson-databind/2.16.1/jackson-databind-2.16.1.pom"),
    ("org.junit.jupiter:junit-jupiter:5.10.1",
     "https://repo1.maven.org/maven2/org/junit/jupiter/junit-jupiter/5.10.1/junit-jupiter-5.10.1.pom"),
    ("org.apache.maven:maven-core:3.9.6",
     "https://repo1.maven.org/maven2/org/apache/maven/maven-core/3.9.6/maven-core-3.9.6.pom"),
    ("org.junit.jupiter:junit-jupiter-api:5.10.1",
     "https://repo1.maven.org/maven2/org/junit/jupiter/junit-jupiter-api/5.10.1/junit-jupiter-api-5.10.1.pom"),
    ("org.slf4j:slf4j-api:2.0.11",
     "https://repo1.maven.org/maven2/org/slf4j/slf4j-api/2.0.11/slf4j-api-2.0.11.pom"),
    ("com.squareup.okhttp3:okhttp:4.12.0",
     "https://repo1.maven.org/maven2/com/squareup/okhttp3/okhttp/4.12.0/okhttp-4.12.0.pom"),
]

# Curated once (HTTP-200 verified) and split across TEI's two in-repo sources.
TEI_URLS = [
    "TEIC/Stylesheets@cda7f87ead3d52629556b1d24726ffc1d042be61|Test/test14.xml",
    "TEIC/Stylesheets@cda7f87ead3d52629556b1d24726ffc1d042be61|Test/test32.xml",
    "TEIC/TEI@884f8dd3c453595742ed706e1ac155db46731338|P5/Test/tei_lite.xml",
    "TEIC/TEI@884f8dd3c453595742ed706e1ac155db46731338|P5/Test/testms.xml",
    "TEIC/Stylesheets@cda7f87ead3d52629556b1d24726ffc1d042be61|Test/test27.xml",
    "TEIC/TEI@884f8dd3c453595742ed706e1ac155db46731338|P5/Test/testnames.xml",
    "TEIC/TEI@884f8dd3c453595742ed706e1ac155db46731338|Documents/ODDintro/2020-version.xml",
    "TEIC/TEI@884f8dd3c453595742ed706e1ac155db46731338|Documents/ODDintro/2013-version.xml",
    "TEIC/TEI@884f8dd3c453595742ed706e1ac155db46731338|Documents/oldODDmanual/oddmanual.xml",
    "TEIC/TEI@884f8dd3c453595742ed706e1ac155db46731338|P5/Source/Guidelines/en/CO-CoreElements.xml",
]

MUSICXML_URLS = [
    "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d|music21/corpus/trecento/PMFC_13_04-Credo Cursor.xml",
    "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d|music21/corpus/trecento/PMFC_13_19-Credo Scabroso.xml",
    "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d|music21/corpus/schumann_clara/opus17/movement3.xml",
    "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d|music21/corpus/schubert/Lindenbaum.xml",
    "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d|music21/corpus/luca/gloria.xml",
    "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d|music21/corpus/bach/bwv69.6.xml",
    "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d|music21/corpus/corelli/opus3no1/1grave.xml",
    "CPJKU/partitura@427ff875bd5a49a0eec894fdd7c6631ed7f597ea|tests/data/musicxml/test_part_group.xml",
    "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d|music21/omr/k525GTMvt1.xml",
    "cuthbertLab/music21@ac6cf27258cbbda118f55470f41adb7a84a6f41d|music21/omr/k525OMRMvt1.xml",
]

DOCBOOK_URLS = [
    "docbook/xslTNG@a840909a8c82d23458ba72e61e0eed4185be6b74|src/test/resources/xml/table-cals.049.xml",
    "docbook/xslTNG@a840909a8c82d23458ba72e61e0eed4185be6b74|src/guide/xml/ref-params.xml",
    "docbook/xslTNG@a840909a8c82d23458ba72e61e0eed4185be6b74|src/guide/xml/ch02.xml",
    "docbook/xslTNG@a840909a8c82d23458ba72e61e0eed4185be6b74|src/test/resources/xml/book.001.xml",
    "docbook/xslTNG@a840909a8c82d23458ba72e61e0eed4185be6b74|src/test/resources/xml/article.001.xml",
    "docbook/xslTNG@a840909a8c82d23458ba72e61e0eed4185be6b74|src/test/resources/xml/refentry.006.xml",
    "docbook/xslTNG@a840909a8c82d23458ba72e61e0eed4185be6b74|src/test/resources/xml/JFK_Inaugural.xml",
    "docbook/xslt10-stylesheets@efd62655c11cc8773708df7a843613fa1e932bf8|testdocs/tests/ebnf/productionset.006.xml",
]


def expand(spec: str) -> str:
    repo_sha, path = spec.split("|", 1)
    repo, sha = repo_sha.split("@")
    return RAW.format(repo=repo, sha=sha, path=urllib.parse.quote(path, safe="/()'+"))


def build():
    E = []

    # --- JATS/PMC (10) -----------------------------------------------------
    PMC = [
        ("PMC10914681", "2024"), ("PMC6398983", "2019"), ("PMC3258128", "2012"),
        ("PMC7074586", "2020"), ("PMC4735955", "2016"), ("PMC4591456", "2015"),
        ("PMC3903843", "2013"), ("PMC13456060", "2026"), ("PMC12900525", "2026"),
        ("PMC13533594", "2026"),
    ]
    for i, (pmc, yr) in enumerate(PMC, 1):
        eid = "jats-%03d" % i
        url = "https://www.ebi.ac.uk/europepmc/webservices/rest/%s/fullTextXML" % pmc
        e = entry(eid, eid + ".xml", "JATS-PMC", "europepmc", url)
        e.update({
            "source_org": "Europe PMC / PubMed Central Open Access subset",
            "source_ref": "EuropePMC:%s" % pmc,
            "retrieval_method": "europepmc-rest-fullTextXML",
            "license": "per-article (see license_url / license_note)",
            "spdx": None,
            "license_url": None,
            "attribution": "Article authors and journal; retrieved via Europe PMC REST",
            "article_pmcid": pmc,
            "article_year": yr,
            "notes": "PMC article licence varies per article; recorded in license_note "
                     "from the article <permissions><license> element.",
        })
        E.append(e)

    # --- SEC/XBRL (8) ------------------------------------------------------
    SEC = [
        ("data/AAPL-Xbrl.2013.xml", 1320000),
        ("data/xbrl/datafiles/msft/msft-20240630_htm.xml", 8900000),
        ("data/frontier_10k_xbrl.xml", 4500000),
        ("data/crr.xbrl.xml", 2270000),
        ("data/Netflix.10K.xbrl.xml", 1530000),
        ("data/xbrl/datafiles/nflx/2024/nflx-20240331_htm.xml", 1140000),
        ("data/xbrl/datafiles/unp/unp-20121231.xml", 2281159),
        ("data/D.1685REIT.xml", 20000),
    ]
    sub = ("SUBSTITUTION: direct SEC EDGAR hosts (www.sec.gov, data.sec.gov) return "
           "HTTP 403 from the retrieval host. Real EDGAR/XBRL filing instances are "
           "retrieved from the MIT-licensed dgunning/edgartools repository, which "
           "mirrors genuine SEC filings; structural diversity preserved.")
    for i, (path, _sz) in enumerate(SEC, 1):
        eid = "secxbrl-%03d" % i
        e = entry(eid, eid + ".xml", "SEC-XBRL", "edgartools",
                  raw("edgartools", path), notes=sub)
        e["attribution"] = "U.S. Securities and Exchange Commission public filing"
        E.append(e)

    # --- OSM (8) -----------------------------------------------------------
    # Chosen to span the §16.10.5 acceleration-crossover range (three mid-size
    # extracts in 32-256 MiB uncompressed, five above 256 MiB) while covering
    # Arabic, Thai, Cyrillic, CJK and Latin script diversity.
    OSM = [
        ("Baghdad", "6.8M"), ("Cairo", "20M"), ("Beirut", "6.7M"),
        ("PhnomPenh", "4.2M"), ("Bangkok", "40M"), ("Moscow", "81M"),
        ("Tokyo", "83M"), ("Warsaw", "117M"),
    ]
    for i, (city, _sz) in enumerate(OSM, 1):
        eid = "osm-%03d" % i
        url = "https://download.bbbike.org/osm/bbbike/%s/%s.osm.gz" % (city, city)
        e = entry(eid, eid + ".osm", "OSM", "osm", url, transform="gzip")
        e.update({
            "source_org": "OpenStreetMap contributors (BBBike city extract)",
            "source_ref": "BBBike:%s@retrieved-%s" % (city, RETRIEVED),
            "retrieval_method": "https-get",
            "attribution": "\u00a9 OpenStreetMap contributors, ODbL 1.0 (BBBike extract)",
            "notes": "SUBSTITUTION: download.geofabrik.de .osm.bz2 returned 404; "
                     "BBBike daily-rolling city extracts used. No immutable revision "
                     "exists, so the snapshot is pinned by SHA-256 + retrieval date.",
        })
        E.append(e)

    # --- DBLP substitute (1) ----------------------------------------------
    e = entry("dblp-001", "dblp-001.xml", "DBLP", "pubmed",
              "https://ftp.ncbi.nlm.nih.gov/pubmed/baseline/pubmed26n0001.xml.gz",
              transform="gzip")
    e.update({
        "source_org": "U.S. National Library of Medicine (PubMed baseline)",
        "source_ref": "pubmed/baseline/pubmed26n0001.xml.gz",
        "attribution": "National Library of Medicine (NLM) PubMed baseline 2026",
        "notes": "SUBSTITUTION: persistent monthly DBLP snapshots "
                 "(dblp.org/xml/release/) are unavailable from the retrieval host "
                 "(anti-bot challenge; direct dump returns an HTML page). This single "
                 "very-large DTD-bearing repetitive bibliographic dataset fills the "
                 "same structural role.",
    })
    E.append(e)

    # --- AOSP (10) ---------------------------------------------------------
    AOSP = [
        "platform/frameworks/base/+/android-14.0.0_r1/core/res/AndroidManifest.xml",
        "platform/packages/apps/Settings/+/android-14.0.0_r1/AndroidManifest.xml",
        "platform/frameworks/base/+/android-14.0.0_r1/packages/SystemUI/AndroidManifest.xml",
        "platform/frameworks/base/+/android-14.0.0_r1/core/res/res/values/strings.xml",
        "platform/frameworks/base/+/android-14.0.0_r1/packages/SystemUI/res/values/strings.xml",
        "platform/frameworks/base/+/android-14.0.0_r1/core/res/res/layout/alert_dialog_material.xml",
        "platform/frameworks/base/+/android-14.0.0_r1/core/res/res/layout/action_bar_home.xml",
        "platform/packages/apps/Settings/+/android-14.0.0_r1/res/values/strings.xml",
        "platform/frameworks/base/+/android-14.0.0_r1/packages/SystemUI/res/xml/lockscreen_settings.xml",
        "platform/frameworks/base/+/android-14.0.0_r1/packages/SystemUI/res/xml/fileprovider.xml",
    ]
    for i, p in enumerate(AOSP, 1):
        eid = "aosp-%03d" % i
        url = "https://android.googlesource.com/%s?format=TEXT" % p
        e = entry(eid, eid + ".xml", "AOSP", "aosp", url, transform="base64")
        e.update({
            "source_org": "Android Open Source Project (android.googlesource.com)",
            "source_ref": REV["aosp"],
            "retrieval_method": "gitiles-format=TEXT (base64)",
            "attribution": "The Android Open Source Project (Apache-2.0)",
        })
        E.append(e)

    # --- Maven (10) --------------------------------------------------------
    for i, (art, url) in enumerate(MAVEN, 1):
        eid = "maven-%03d" % i
        e = entry(eid, eid + ".pom.xml", "MAVEN", "maven", url)
        e.update({
            "source_org": "Maven Central (repo1.maven.org)",
            "source_ref": art,
            "retrieval_method": "https-get",
            "license": "per-artifact (see license_note)",
            "spdx": None,
            "license_url": None,
            "notes": "Licence extracted from the POM <licenses> element.",
        })
        E.append(e)

    # --- TEI (10) ----------------------------------------------------------
    for i, spec in enumerate(TEI_URLS, 1):
        eid = "tei-%03d" % i
        url = expand(spec)
        src = "tei" if "/TEIC/TEI/" in url else "tei_stylesheets"
        E.append(entry(eid, eid + ".xml", "TEI", src, url))

    # --- MusicXML (10) -----------------------------------------------------
    for i, spec in enumerate(MUSICXML_URLS, 1):
        eid = "musicxml-%03d" % i
        url = expand(spec)
        src = "partitura" if "/CPJKU/partitura/" in url else "music21"
        E.append(entry(eid, eid + ".musicxml", "MUSICXML", src, url))

    # --- SVG (8) -----------------------------------------------------------
    SVG = [
        ("resvg", "crates/resvg/tests/resources/simple-text.svg"),
        ("resvg", "crates/resvg/tests/extra/horizontal-line.svg"),
        ("resvg", "crates/resvg/tests/resources/image.svg"),
        ("batik", "samples/barChart.svg"),
        ("batik", "samples/3D.svg"),
        ("batik", "samples/asf-logo.svg"),
        ("batik", "contrib/fonts/gladiator/svg/glb12.svg"),
        ("batik", "samples/mapWaadt.svg"),
    ]
    for i, (src, path) in enumerate(SVG, 1):
        E.append(entry("svg-%03d" % i, "svg-%03d.svg" % i, "SVG", src, raw(src, path)))

    # --- DocBook (8) -------------------------------------------------------
    for i, spec in enumerate(DOCBOOK_URLS, 1):
        eid = "docbook-%03d" % i
        url = expand(spec)
        src = "xslt10" if "/xslt10-stylesheets/" in url else "xsltng"
        E.append(entry(eid, eid + ".xml", "DOCBOOK", src, url))

    # --- GPX/KML (7) -------------------------------------------------------
    GPX = [
        ("gdal", "autotest/ogr/data/kml/folder_with_subfolder_placemark.kml", "KML"),
        ("libkml", "testdata/gx/all-gx.kml", "KML"),
        ("gdal", "autotest/ogr/data/kml/samples.kml", "KML"),
        ("libkml", "testdata/kml/gnis-ak-first-101.kml", "KML"),
        ("gpsbabel", "reference/osm-data.gpx", "GPX"),
        ("gpsbabel", "reference/skytraq-miniHomer2_8.gpx", "GPX"),
        ("gpsbabel", "reference/lowrance-v4.gpx", "GPX"),
    ]
    for i, (src, path, kind) in enumerate(GPX, 1):
        ext = ".kml" if kind == "KML" else ".gpx"
        e = entry("gpxkml-%03d" % i, "gpxkml-%03d%s" % (i, ext), "GPX-KML", src,
                  raw(src, path))
        e["attribution"] = "GPSBabel/GDAL/libkml test data (see licence)"
        E.append(e)

    # --- RSS/Atom (5) ------------------------------------------------------
    RSS = [
        ("feedvalidator", "testcases/rss/must/rss20_spec_sample_noerror.xml", "RSS 2.0"),
        ("feedvalidator", "testcases/rss20/data-types-datetime/everything.xml", "RSS 2.0"),
        ("feedparser", "tests/wellformed/sanitize/large_atom_feed_that_needs_css_sanitisation.xml", "Atom"),
        ("feedparser", "docs/examples/examples/atom10.xml", "Atom 1.0"),
        ("feedparser", "docs/examples/examples/rss10.rdf", "RSS 1.0 (RDF)"),
    ]
    for i, (src, path, kind) in enumerate(RSS, 1):
        ext = ".rdf" if path.endswith(".rdf") else ".xml"
        E.append(entry("rssatom-%03d" % i, "rssatom-%03d%s" % (i, ext), "RSS-ATOM",
                       src, raw(src, path)))

    # --- SOAP/WSDL/XSD (5) -------------------------------------------------
    SOAP = [
        ("springws", "spring-ws-core/src/main/resources/org/springframework/ws/config/web-services-2.0.xsd", "XSD"),
        ("springws", "spring-ws-core/src/test/resources/org/springframework/ws/client/support/destination/simple.wsdl", "WSDL"),
        ("cxf", "core/src/test/resources/wsdl/foo.wsdl", "WSDL"),
        ("cxf", "distribution/src/main/release/samples/performance/soap_http_doc_lit/src/main/resources/wsdl/perf_policy.wsdl", "WSDL"),
        ("xsdtests", "common/xsts.xsd", "XSD"),
    ]
    for i, (src, path, kind) in enumerate(SOAP, 1):
        ext = ".wsdl" if kind == "WSDL" else ".xsd"
        E.append(entry("soapwsdl-%03d" % i, "soapwsdl-%03d%s" % (i, ext), "SOAP-WSDL-XSD",
                       src, raw(src, path)))

    return E


# --------------------------------------------------------------------------
# Per-file licence extraction for the two variable-licence categories.
# --------------------------------------------------------------------------

def http_get(url: str, timeout=60) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    with OPENER.open(req, timeout=timeout) as r:
        return r.read()


def jats_license(pmc: str) -> str:
    try:
        raw = http_get("https://www.ebi.ac.uk/europepmc/webservices/rest/%s/fullTextXML" % pmc)
    except Exception as exc:  # noqa: BLE001
        return "unavailable (%s)" % exc
    txt = raw.decode("utf-8", "replace")
    m = re.search(r"<license\b[^>]*>(.*?)</license>", txt, re.S)
    if not m:
        m2 = re.search(r"<license\b([^>]*)>", txt)
        return ("license-type=%s" % m2.group(1).strip()) if m2 else "absent"
    body = m.group(0)
    attrs = re.search(r"<license\b([^>]*)>", body).group(1)
    href = re.search(r'xlink:href="([^"]+)"', attrs)
    ltype = re.search(r'license-type="([^"]+)"', attrs)
    p = re.search(r"<license-p>(.*?)</license-p>", body, re.S)
    ptxt = re.sub(r"<[^>]+>", " ", p.group(1)) if p else ""
    ptxt = re.sub(r"\s+", " ", ptxt).strip()
    parts = []
    if ltype:
        parts.append("license-type=%s" % ltype.group(1))
    if href:
        parts.append(href.group(1))
    if ptxt:
        parts.append(ptxt[:220])
    # Many articles carry the machine-readable CC URL outside <license>
    # (e.g. in an <ali:license_ref>); scan the whole document for it.
    if "creativecommons.org/licenses/" not in " ".join(parts):
        cc = re.search(r"creativecommons\.org/licenses/[A-Za-z0-9._/-]+", txt)
        if cc:
            parts.append(cc.group(0))
    return "; ".join(parts) or "present"


def _pom_licenses(txt: str):
    m = re.search(r"<licenses>(.*?)</licenses>", txt, re.S)
    if not m:
        return (None, None)
    names = re.findall(r"<name>(.*?)</name>", m.group(1), re.S)
    urls = re.findall(r"<url>(.*?)</url>", m.group(1), re.S)
    name = "; ".join(re.sub(r"\s+", " ", n).strip() for n in names)
    lurl = next((re.sub(r"\s+", " ", u).strip() for u in urls if u.strip()), None)
    return (name or "unlabelled", lurl)


def maven_license(url: str):
    """Return (license_text, license_url, provenance_note).

    Licences are frequently declared in the artifact's parent POM rather than
    the artifact POM itself (Maven inheritance), so follow <parent> within
    Maven Central up to a small depth and record where the licence came from."""
    cur = url
    trail = []
    for _ in range(4):
        try:
            txt = http_get(cur).decode("utf-8", "replace")
        except Exception as exc:  # noqa: BLE001
            return ("unavailable (%s)" % exc, None, "; ".join(trail))
        name, lurl = _pom_licenses(txt)
        if name:
            note = ("inherited from %s" % trail[-1]) if trail else "declared in artifact POM"
            return (name, lurl, note)
        pm = re.search(
            r"<parent>\s*<groupId>(.*?)</groupId>\s*<artifactId>(.*?)</artifactId>\s*"
            r"<version>(.*?)</version>", txt, re.S)
        if not pm:
            return ("not declared in POM or parent chain", None, "; ".join(trail))
        g, a, v = (re.sub(r"\s+", "", x) for x in pm.groups())
        purl = "https://repo1.maven.org/maven2/%s/%s/%s/%s-%s.pom" % (
            g.replace(".", "/"), a, v, a, v)
        trail.append("%s:%s:%s" % (g, a, v))
        cur = purl
        time.sleep(0.3)
    return ("not declared in POM or parent chain", None, "; ".join(trail))


SPDX_HINTS = [
    ("apache license, version 2.0", "Apache-2.0"),
    ("apache-2.0", "Apache-2.0"),
    ("apache software license", "Apache-2.0"),
    ("eclipse public license - v 2.0", "EPL-2.0"),
    ("eclipse public license v2.0", "EPL-2.0"),
    ("the mit license", "MIT"),
    ("mit license", "MIT"),
    ("bouncy castle licence", "MIT"),
    ("bsd 3-clause", "BSD-3-Clause"),
    ("bsd 2-clause", "BSD-2-Clause"),
    ("go license", "BSD-3-Clause"),
]


def spdx_from_text(text: str):
    low = (text or "").lower()
    for needle, spdx in SPDX_HINTS:
        if needle in low:
            return spdx
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true", help="HEAD-verify every URL")
    ap.add_argument("--no-licence", action="store_true",
                    help="skip the network licence-extraction pass")
    args = ap.parse_args()

    entries = build()
    ids = [e["id"] for e in entries]
    assert len(ids) == len(set(ids)), "duplicate ids"
    print("entries:", len(entries))

    if not args.no_licence:
        for e in entries:
            if e["category"] == "JATS-PMC":
                note = jats_license(e["article_pmcid"])
                e["license_note"] = note
                m = re.search(r"creativecommons\.org/licenses/([a-z-]+)/([\d.]+)", note or "")
                if m:
                    e["license"] = "CC-%s-%s" % (m.group(1).upper(), m.group(2))
                    e["spdx"] = "CC-%s-%s" % (m.group(1).upper(), m.group(2))
                    e["license_url"] = "https://" + m.group(0)
                elif "license-type" in (note or ""):
                    e["license"] = note.split(";")[0]
                time.sleep(0.5)
            elif e["category"] == "MAVEN":
                name, lurl, note = maven_license(e["source_url"])
                e["license"] = name
                e["license_url"] = lurl
                e["spdx"] = spdx_from_text(name)
                e["license_note"] = "%s (%s)" % (name, note)
                time.sleep(0.3)

    if args.check:
        for e in entries:
            u = e["source_url"]
            try:
                req = urllib.request.Request(u, headers={"User-Agent": UA}, method="HEAD")
                with OPENER.open(req, timeout=45) as r:
                    cl = r.headers.get("Content-Length")
                    print("  %-14s %s %s" % (e["id"], r.status, cl))
            except Exception as exc:  # noqa: BLE001
                print("  %-14s FAIL %s" % (e["id"], exc))
            time.sleep(0.4)

    # `--check` is a verification-only mode: it must never clobber the sealed
    # manifest (which carries the fetch-recorded hashes) with a pre-fetch copy.
    if args.check:
        print("--check: manifest NOT rewritten (verification-only mode)")
        return 0

    with open(MANIFEST, "w", encoding="utf-8") as f:
        json.dump({"schema": "corpus-manifest/1", "phase": "16.10",
                   "retrieved_at": RETRIEVED, "entries": entries}, f,
                  indent=1, ensure_ascii=False)
        f.write("\n")
    print("wrote", MANIFEST)
    return 0


if __name__ == "__main__":
    sys.exit(main())
