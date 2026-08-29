#!/bin/bash
# ── Historical behavior matrix runner (§41, §42, §51) ────────────────────
# Runs a fixed behavioral corpus across every built historical oracle and
# the system oracle, capturing stdout/stderr/exit for each case. Outputs:
#   results/<version>/<case>.out/.err/.exit        raw captures
#   results/<version>/fingerprint.json             per-case sha256 + version identity
#   matrix.json                                    version -> case -> hash
#
# Usage: run_matrix.sh [tool]   (tool = xmllint | xsltproc; default: both)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$ROOT/oracle/historical"
CORPUS="$OUT/corpus"
RES="$OUT/results"
TOOL="${1:-all}"

# ── Oracle registry ───────────────────────────────────────────────────────
# Built historical libxml2 anchors (2.6.32 excluded: era toolchain required).
XML2_VERSIONS="2.7.8 2.8.0 2.9.4 2.9.10 2.9.14 2.10.4 2.11.5 2.12.6 2.13.5 2.14.1"
XSLT_PAIRS="1.1.26:2.7.8 1.1.32:2.9.4 1.1.35:2.9.10 1.1.38:2.10.4 1.1.42:2.11.5"

hash_case() { # version case exit_file out_file err_file
  local v="$1" c="$2" xf="$3" of="$4" ef="$5"
  local h
  h="$(cat "$xf" "$of" "$ef" 2>/dev/null | sha256sum | cut -d' ' -f1)"
  echo "$h"
}

run_case() { # tool version name argv...
  local tool="$1" v="$2" c="$3"; shift 3
  local vdir="$RES/$tool/$v"
  mkdir -p "$vdir"
  if [ "$tool" = xmllint ]; then
    local bin
    if [ "$v" = system ]; then bin="/usr/bin/xmllint"; else bin="$OUT/prefix/libxml2-$v/bin/xmllint"; fi
    [ -x "$bin" ] || { echo "  skip $v/$c (no binary)"; return; }
    "$bin" "$@" >"$vdir/$c.out" 2>"$vdir/$c.err" || true
  else
    local bin
    if [ "$v" = system ]; then bin="/usr/bin/xsltproc"; else bin="$OUT/prefix/libxslt-$v/bin/xsltproc"; fi
    [ -x "$bin" ] || { echo "  skip $v/$c (no binary)"; return; }
    # xsltproc cases: last arg is the document, preceded by stylesheet
    "$bin" "$@" >"$vdir/$c.out" 2>"$vdir/$c.err" || true
  fi
  echo "$?" > "$vdir/$c.exit"
}

cd "$CORPUS"

if [ "$TOOL" = all ] || [ "$TOOL" = xmllint ]; then
  echo "═══ xmllint historical matrix ═══"
  for v in $XML2_VERSIONS system; do
    echo "── $v ──"
    run_case xmllint "$v" version        --version
    run_case xmllint "$v" dump-simple    simple.xml
    run_case xmllint "$v" dump-empty     empty.xml
    run_case xmllint "$v" dump-dtd       dtd.xml
    run_case xmllint "$v" format-dtd     --format dtd.xml
    run_case xmllint "$v" parse-error    bad.xml
    run_case xmllint "$v" valid-invalid  --valid invalid.xml
    run_case xmllint "$v" valid-nodtd    --valid simple.xml
    run_case xmllint "$v" noent          --noent ent.xml
    run_case xmllint "$v" noent-decl     --noent dclent.xml
    run_case xmllint "$v" undeclared     undeclared.xml
    run_case xmllint "$v" attr-entity    attrent.xml
    run_case xmllint "$v" xpath-nodeset  lib.xml --xpath '//book'
    run_case xmllint "$v" xpath-string   lib.xml --xpath 'string(//title)'
    run_case xmllint "$v" xpath-count    lib.xml --xpath 'count(//book)'
    run_case xmllint "$v" xpath-attr     lib.xml --xpath '@id'
    run_case xmllint "$v" debug-simple   --debug simple.xml
    run_case xmllint "$v" debug-dtd      --debug dtd.xml
    run_case xmllint "$v" debug-nodes    --debug nodes.xml
    run_case xmllint "$v" debug-longtext --debug longtext.xml
    run_case xmllint "$v" debug-ns       --debug ns.xml
    run_case xmllint "$v" html-dump      --html page.html
    run_case xmllint "$v" html-debug     --html --debug page.html
    run_case xmllint "$v" c14n           --c14n lib.xml
    run_case xmllint "$v" copy-dtd       --copy dtd.xml
    run_case xmllint "$v" dropdtd        --dropdtd dtd.xml
  done
fi

if [ "$TOOL" = all ] || [ "$TOOL" = xsltproc ]; then
  echo "═══ xsltproc historical matrix ═══"
  # Build a small stylesheet corpus in the results dir.
  STYLES="$OUT/styles"
  mkdir -p "$STYLES"
  cat > "$STYLES/basic.xsl" <<'XEOF'
<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml" indent="yes"/>
  <xsl:template match="/"><out><xsl:apply-templates select="//item"/></out></xsl:template>
  <xsl:template match="item"><it><xsl:value-of select="."/></it></xsl:template>
</xsl:stylesheet>
XEOF
  cat > "$STYLES/num.xsl" <<'XEOF'
<?xml version="1.0"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:output method="xml"/>
  <xsl:template match="/"><out><xsl:for-each select="//item"><n><xsl:value-of select="position()"/></n></xsl:for-each></out></xsl:template>
</xsl:stylesheet>
XEOF
  printf '<r><item>one</item><item>two</item><item>three</item></r>' > "$CORPUS/items.xml"
  for pair in $XSLT_PAIRS; do
    v="${pair%%:*}"; xmlv="${pair##*:}"
    echo "── $v (vs libxml2-$xmlv) ──"
    run_case xsltproc "$v" basic  "$STYLES/basic.xsl" items.xml
    run_case xsltproc "$v" num    "$STYLES/num.xsl" items.xml
    run_case xsltproc "$v" empty  "$STYLES/basic.xsl" simple.xml
  done
  echo "── system ──"
  run_case xsltproc system basic "$STYLES/basic.xsl" items.xml
  run_case xsltproc system num   "$STYLES/num.xsl" items.xml
  run_case xsltproc system empty "$STYLES/basic.xsl" simple.xml
fi

# ── Fingerprint + matrix emission ────────────────────────────────────────
python3 - "$RES" "$TOOL" <<'PYEOF'
import hashlib, json, os, sys
res, tool = sys.argv[1], sys.argv[2]
matrix = {}
tools = ["xmllint", "xsltproc"] if tool == "all" else [tool]
for t in tools:
    tdir = os.path.join(res, t)
    if not os.path.isdir(tdir):
        continue
    versions = sorted(os.listdir(tdir))
    for v in versions:
        vdir = os.path.join(tdir, v)
        fp = {"version": v, "cases": {}}
        for name in sorted(os.listdir(vdir)):
            if not name.endswith(".exit"):
                continue
            base = name[:-5]
            blob = b""
            for ext in (".exit", ".out", ".err"):
                p = os.path.join(vdir, base + ext)
                if os.path.exists(p):
                    blob += open(p, "rb").read()
            fp["cases"][base] = hashlib.sha256(blob).hexdigest()
        with open(os.path.join(vdir, "fingerprint.json"), "w") as f:
            json.dump(fp, f, indent=1)
        matrix.setdefault(t, {})[v] = fp["cases"]
out = os.path.join(res, "matrix.json")
with open(out, "w") as f:
    json.dump(matrix, f, indent=1, sort_keys=True)
print("matrix written:", out)

# Epoch grouping: for each case, group versions by identical hash.
for t, versions in matrix.items():
    print(f"\n── {t} semantic epochs (per-case identical outputs) ──")
    all_cases = sorted({c for v in versions.values() for c in v})
    for case in all_cases:
        groups = {}
        for v, cases in versions.items():
            h = cases.get(case, "MISSING")
            groups.setdefault(h, []).append(v)
        if len(groups) > 1:
            print(f"  {case}:")
            for h, vs in groups.items():
                print(f"    {h[:12]}  {', '.join(vs)}")
PYEOF
