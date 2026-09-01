#!/bin/bash
# php-run.sh — Phase 14 PHP court, in-container runner.
#
# Usage: php-run.sh <oracle|candidate>
# Builds the pinned PHP 8.5.10 with the XML-facing extensions (dom, simplexml,
# xml, xmlreader, xmlwriter, xsl) against the selected libxml2 via pkg-config,
# runs the extension test suites, writes:
#   /out/<mode>-configure.log /out/<mode>-build.log /out/<mode>-libver.txt
#   /out/<mode>-tests.log /out/<mode>-result.json
set -uo pipefail
MODE="${1:?usage: php-run.sh <oracle|candidate>}"
source /court/consumers/lib.sh "$MODE"

cd /src
rm -rf php-build && mkdir php-build && tar xf php-8.5.10.tar.gz -C php-build --strip-components=1
cd php-build

# ── configure (libxml discovered via pkg-config) ──────────────────────────
./configure --prefix=/usr/local/php --disable-all --enable-cli \
    --enable-dom --enable-simplexml --enable-xml --enable-xmlreader \
    --enable-xmlwriter --enable-xsl --with-libxml \
    > "/out/${MODE}-configure.log" 2>&1
CONF_RC=$?

if [ "$CONF_RC" -ne 0 ]; then
    echo "{\"consumer\":\"php\",\"mode\":\"$MODE\",\"build_ok\":false,\"configure_tail\":\"$(tail -c 2000 /out/${MODE}-configure.log | tr '\n' ' ' | sed 's/"/\\"/g')\"}" \
        > "/out/${MODE}-result.json"
    exit 0
fi

make -j"$(nproc)" > "/out/${MODE}-build.log" 2>&1
BUILD_RC=$?

# libxml version actually used
sapi/cli/php -r 'echo "LIBXML_DOTTED_VERSION=", LIBXML_DOTTED_VERSION, "\n"; echo "libxml2 lib: ", (class_exists("DOMDocument") ? "ok" : "missing"), "\n";' \
    > "/out/${MODE}-libver.txt" 2>&1 || true
ldd sapi/cli/php 2>/dev/null | grep -E "libxml2|libxslt|libexslt" \
    | sed 's/^[[:space:]]*//;s/=>.*//' > "/out/${MODE}-ldd.txt" || true

if [ "$BUILD_RC" -ne 0 ]; then
    echo "{\"consumer\":\"php\",\"mode\":\"$MODE\",\"build_ok\":false,\"build_tail\":\"$(tail -c 2000 /out/${MODE}-build.log | tr '\n' ' ' | sed 's/"/\\"/g')\"}" \
        > "/out/${MODE}-result.json"
    exit 0
fi

# ── extension test suites ──────────────────────────────────────────────────
make test TESTS="ext/dom ext/simplexml ext/xml ext/xmlreader ext/xmlwriter ext/xsl" \
    NO_INTERACTION=1 REPORT_EXIT_STATUS=1 > "/out/${MODE}-tests.log" 2>&1
TEST_RC=$?

python3 - "$MODE" "$TEST_RC" <<'PYEOF' > "/out/${MODE}-result.json"
import json
import re
import sys

mode, test_rc = sys.argv[1], int(sys.argv[2])
log = open(f"/out/{mode}-tests.log", encoding="utf-8", errors="replace").read()
exts = {}
for ext in ("dom", "simplexml", "xml", "xmlreader", "xmlwriter", "xsl"):
    m = re.search(rf"{ext}:.*?Tests:? (\d+).*?Failures:? (\d+).*?Skipped:? (\d+)",
                  log, re.S)
    if m:
        exts[ext] = {"tests": int(m.group(1)), "failures": int(m.group(2)),
                     "skipped": int(m.group(3))}
    else:
        m2 = re.search(rf"{ext}.*?(\d+) / (\d+) tests? (?:\((\d+) skipped\))?",
                       log)
        if m2:
            exts[ext] = {"tests": int(m2.group(1)), "failures": int(m2.group(2)),
                         "skipped": int(m2.group(3) or 0)}
# global summary line "Number of tests:  N"
m = re.search(r"Number of tests:\s+(\d+).*?Tests skipped:\s+(\d+).*?(?:Failures:\s+(\d+))?",
              log, re.S)
summary = None
if m:
    summary = {"tests": int(m.group(1)), "skipped": int(m.group(2)),
               "failures": int(m.group(3) or 0)}
print(json.dumps({
    "consumer": "php", "mode": mode, "build_ok": True, "test_rc": test_rc,
    "ext_summary": exts, "summary": summary,
    "tests_tail": log[-1500:] or "",
}))
PYEOF

echo "php-run ${MODE} done rc=${CONF_RC}/${BUILD_RC}/${TEST_RC}"
