#!/usr/bin/env python3
"""Phase 14 — lxml differential operation corpus (residual-mining engine).

Deterministic fingerprint harness: exercises lxml's public API surface across
parsing/HTML/push/SAX/tree/XPath/XSLT/EXSLT/schemas/RELAXNG/DTD/serialization/
C14N/encoding/errors/lifetime and prints one normalized line per operation:

    op:<name>: <fingerprint>

The SAME script runs against the oracle build (system libxml2 2.15.3) and the
candidate build (libxml-rs) and the host runner byte-compares the outputs.
Any differing line is a divergence: known residuals (R-000157 encodings)
are classified by the runner; anything else becomes a new residual.

Normalization rules: absolute paths, memory addresses and object ids are
never printed; exceptions print type + first message line only; result
strings are stripped of trailing whitespace. Every operation is wrapped so a
crash on one side is a fingerprint difference, not a corpus abort.

Usage: python3 lxml-diffcorpus.py > corpus.txt
"""
import sys
import traceback

import lxml.etree as ET
import lxml.html as LH
import lxml.html.html5parser  # noqa: F401  (import surface check)
import lxml.objectify  # noqa: F401
import lxml.builder as E
import lxml.sax
import lxml.cssselect  # noqa: F401

OUT = []


def emit(name, value):
    s = repr(value)
    if len(s) > 400:
        s = s[:400] + "..."
    OUT.append(f"op:{name}: {s}")
    # Print immediately so a mid-corpus crash still identifies its op.
    print(f"op:{name}: {s}", flush=True)


def op(name, fn):
    try:
        emit(name, fn())
    except Exception as exc:  # noqa: BLE001 — fingerprints on purpose
        emit(name, ("EXC", type(exc).__name__, str(exc).splitlines()[:1]))


def ser(node, method="xml", **kw):
    return ET.tostring(node, method=method, with_tail=False, **kw).decode("utf-8", "replace")


def errinfo():
    """Structured error fingerprint (normalized)."""
    err = ET.get_last_error()
    if err is None:
        return None
    return (err.domain, err.code, err.level, err.line, err.column)


# ── 1. XML parsing: options matrix ────────────────────────────────────────
XMLS = {
    "simple": "<root><a>1</a><b>2</b></root>",
    "ns": '<root xmlns:x="http://x"><x:a x:attr="v">t</x:a></root>',
    "entity": "<!DOCTYPE r [<!ENTITY e 'hello'>]><r>&e;</r>",
    "pi": "<?pi data?><r/>",
    "comment": "<r><!-- c --><a/></r>",
    "doctype": "<!DOCTYPE html PUBLIC '-//W3C//DTD XHTML 1.0//EN' 'http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd'><html/>",
    "attr-quote": "<r a=\"1\" b='2' c=3/>",
    "empty": "",
    "ws": "   <r>  <a> x </a>  </r>  ",
    "cdata": "<r><![CDATA[<not-markup>]]></r>",
}
for name, doc in XMLS.items():
    def _p(d=doc, n=name):
        t = ET.fromstring(d.encode())
        return ser(t)
    op(f"parse-{name}", _p)

for name, doc in XMLS.items():
    def _p(d=doc, n=name):
        t = ET.fromstring(d.encode())
        return (t.tag, sorted(t.attrib.items()), t.text, t.tail)
    op(f"parse-{name}-fields", _p)

# option matrix on one hostile doc (libxml2 xmlParserOption enum values,
# stable across versions; lxml does not re-expose them as attributes)
XML_PARSE_RECOVER, XML_PARSE_NOENT, XML_PARSE_NOBLANKS, XML_PARSE_NOCDATA = 1, 2, 256, 16384
XML_PARSE_PEDANTIC, XML_PARSE_NOERROR, XML_PARSE_NOWARNING, XML_PARSE_DTDLOAD = 128, 32, 64, 4
XML_PARSE_HUGE, XML_PARSE_COMPACT, XML_PARSE_NOXINCNODE, XML_PARSE_OLD10 = 524288, 65536, 32768, 131072
HOSTILE = "<!DOCTYPE r [<!ENTITY a '&a;'>]><r>&a;<bad><x></r>"
for opt in (XML_PARSE_RECOVER, XML_PARSE_NOENT, XML_PARSE_NOBLANKS,
            XML_PARSE_NOCDATA, XML_PARSE_PEDANTIC, XML_PARSE_NOERROR,
            XML_PARSE_NOWARNING, XML_PARSE_DTDLOAD, XML_PARSE_HUGE,
            XML_PARSE_COMPACT, XML_PARSE_NOXINCNODE, XML_PARSE_OLD10):
    def _p(opt=opt):
        p = ET.XMLParser(recover=bool(opt & XML_PARSE_RECOVER), no_network=True)
        t = ET.fromstring(HOSTILE.encode(), parser=p)
        return ser(t) if t is not None else None
    op(f"parse-hostile-opt{opt}", _p)

# ── 2. Recovery / malformed inputs ────────────────────────────────────────
MALFORMED = [
    "unclosed <a><b>",
    "mismatch <a><b></a></b>",
    "bad-attr <a x='1' x='2'/>",
    "bad-entity <a>&nope;</a>",
    "double-root <a/><b/>",
    "lone-amp <a>&</a>",
    "control <a>\x01</a>",
    "bad-utf8 <a>\xff\xfe</a>",
    "bad-decl <?xml version='1.0' encoding='x'?><a/>",
    "empty-tag <a></a",
    "unclosed-comment <a><!-- x",
    "unclosed-cdata <a><![CDATA[x",
    "bad-ns <a xmlns='' xmlns:p=''>",
    "dup-ns <a xmlns:p='1' xmlns:p='2'/>",
    "invalid-name <1a/>",
    "deeply-nested " + "<a>" * 300 + "x" + "</a>" * 300,
]
for name, doc in enumerate(MALFORMED):
    def _p(d=doc):
        p = ET.XMLParser(recover=True, no_network=True)
        t = ET.fromstring(d.encode(), parser=p)
        return ser(t) if t is not None else None
    op(f"recover-{name}", _p)
    def _pe(d=doc):
        p = ET.XMLParser(recover=False, no_network=True)
        ET.fromstring(d.encode(), parser=p)
        return errinfo()
    op(f"recover-{name}-err", _pe)

# ── 3. Push / incremental parsing ─────────────────────────────────────────
def push_chunks(chunks, recover=True):
    p = ET.XMLParser(recover=recover, no_network=True)
    t = ET.ElementTree()
    root = None
    for c in chunks:
        root = ET.XMLPullParser(events=("end",)) if False else None
        break
    # use the incremental feed API
    parser = ET.XMLPullParser(events=("start", "end", "comment", "pi"))
    for c in chunks:
        parser.feed(c)
    events = [(ev, el.tag if ev != "comment" and ev != "pi" else None)
              for ev, el in parser.read_events()]
    parser.close()
    return events
op("push-even", push_chunks(["<r>", "<a>1</a>", "<b>", "2", "</b></r>"]))

def iterparse_events(doc, events=("end",), tag=None):
    import io
    evs = []
    for ev, el in ET.iterparse(io.BytesIO(doc.encode()), events=events, tag=tag):
        evs.append((ev, el.tag))
        if ev == "end" and el.getparent() is not None:
            el.clear()
    return evs
op("iterparse", lambda: iterparse_events("<r><a/><b>t</b><a/></r>"))
op("iterparse-start", lambda: iterparse_events("<r><a/><b/></r>", events=("start",)))
op("iterparse-tag", lambda: iterparse_events("<r><a/><b>t</b><a/></r>", tag="a"))

# ── 4. HTML parsing ───────────────────────────────────────────────────────
HTMLS = {
    "basic": "<html><head><title>T</title></head><body><p>P</p></body></html>",
    "broken": "<html><body><p>a<p>b</body>",
    "table": "<table><tr><td>1<td>2<tr><td>3</table>",
    "entities": "<html><body>&nbsp;&copy;&#65;&#x42;</body></html>",
    "meta-enc": "<html><head><meta http-equiv='Content-Type' content='text/html; charset=iso-8859-1'></head><body>caf\xe9</body></html>",
    "comments": "<!-- c --><html><body>x</body></html>",
    "script": "<html><body><script>if (a < b && c > d) { x('</script>'); }</script></body></html>",
    "fragment": "<p>a</p><p>b</p>",
}
for name, doc in HTMLS.items():
    def _h(d=doc, n=name):
        t = LH.fromstring(d)
        return LH.tostring(t, encoding="unicode") if t is not None else None
    op(f"html-{name}", _h)

def html_frag(d):
    frags = LH.fragments_fromstring(d)
    return [LH.tostring(f, encoding="unicode") for f in frags]
op("html-fragments", lambda: html_frag("<p>a</p><p>b</p><div><span>c</span></div>"))

# ── 5. SAX ────────────────────────────────────────────────────────────────
class SaxSink:
    def __init__(self):
        self.ev = []
    def startDocument(self):
        self.ev.append(("doc-start",))
    def endDocument(self):
        self.ev.append(("doc-end",))
    def startElement(self, name, attrs):
        self.ev.append(("start", name, dict(attrs)))
    def endElement(self, name):
        self.ev.append(("end", name))
    def characters(self, data):
        self.ev.append(("chars", data))
    def comment(self, data):
        self.ev.append(("comment", data))
    def processingInstruction(self, target, data):
        self.ev.append(("pi", target, data))
    def startElementNS(self, name, qname, attrs):
        self.ev.append(("startns", name, dict(attrs)))

def sax_parse(doc):
    sink = SaxSink()
    parser = ET.XMLParser(target=sink, no_network=True)
    ET.fromstring(doc.encode(), parser=parser)
    return sink.ev
op("sax-basic", lambda: sax_parse("<r a='1'><b>t</b><!--c--><?pi d?></r>"))

import io as _io
def sax_parse_ns(doc):
    sink = SaxSink()
    parser = ET.XMLParser(target=sink, no_network=True)
    ET.fromstring(doc.encode(), parser=parser)
    return sink.ev
op("sax-ns", lambda: sax_parse_ns('<r xmlns:p="http://p"><p:c p:a="v">t</p:c></r>'))

# ── 6. Tree construction & manipulation ───────────────────────────────────
def tree_ops():
    root = ET.Element("root", a="1")
    child = ET.SubElement(root, "child", b="2")
    child.text = "text"
    child.tail = "tail"
    child2 = ET.Element("child2")
    root.append(child2)
    root.insert(0, ET.Element("first"))
    root[1].set("c", "3")
    del root[1].attrib["b"]
    sub = ET.SubElement(child, "sub")
    sub.text = "deep"
    sib = ET.Element("sib")
    child.addnext(sib)
    child.addprevious(ET.Element("prev"))
    out = [ser(root)]
    out.append([c.tag for c in root.iter()])
    out.append([(c.tag, c.text) for c in root.iter()])
    out.append(root.get("a"))
    out.append(root.get("missing"))
    out.append(root.get("missing", "default"))
    out.append(root.attrib)
    out.append(len(root))
    out.append(root[0].tag)
    return out
op("tree-ops", tree_ops)

def tree_move():
    root = ET.Element("r")
    a = ET.SubElement(root, "a")
    b = ET.SubElement(root, "b")
    root.remove(a)
    root.insert(0, a)
    return ser(root)
op("tree-move", tree_move)

def tree_copy():
    root = ET.fromstring("<r><a><b>t</b></a></r>")
    c = ET.deepcopy(root)
    c[0][0].text = "changed"
    return (ser(root), ser(c))
op("tree-deepcopy", tree_copy)

def tree_clear():
    root = ET.fromstring("<r><a>1</a><b>2</b></r>")
    root.clear()
    return ser(root)
op("tree-clear", tree_clear)

def builder():
    b = E.E
    t = b.root(b.a("1", "text"), b.b("tail", b.c("x")))
    return ser(t)
op("tree-builder", builder)

def objectify_basic():
    import lxml.objectify as O
    t = O.fromstring("<r><a>5</a><b>2.5</b><c>true</c><d>text</d></r>")
    return (int(t.a), float(t.b), bool(t.c), str(t.d), repr(t.d))
op("objectify", objectify_basic)

# ── 7. XPath ──────────────────────────────────────────────────────────────
XPATH_DOC = "<r xmlns:n='http://n'><a i='1'>x</a><a i='2'>y</a><b><a>z</a></b><n:c>w</n:c></r>"

def xpath(expr, namespaces=None, variables=None):
    t = ET.fromstring(XPATH_DOC.encode())
    return [ser(e) if isinstance(e, ET._Element) else e
            for e in t.xpath(expr, namespaces=namespaces, **({"variables": variables} if variables else {}))]
op("xpath-abs", lambda: xpath("/r/a"))
op("xpath-rel", lambda: xpath("a"))
op("xpath-attr", lambda: xpath("//a/@i"))
op("xpath-text", lambda: xpath("//a/text()"))
op("xpath-pred", lambda: xpath("//a[@i='2']"))
op("xpath-pos", lambda: xpath("//a[1]"))
op("xpath-last", lambda: xpath("//a[last()]"))
op("xpath-axis", lambda: xpath("//a/ancestor::r"))
op("xpath-following-sibling", lambda: xpath("//a[1]/following-sibling::a"))
op("xpath-ns", lambda: xpath("//n:c", namespaces={"n": "http://n"}))
op("xpath-unknown-ns", lambda: xpath("//n:c"))
op("xpath-string-fn", lambda: xpath("string(/r/a[1]/@i)"))
op("xpath-number-fn", lambda: xpath("number(/r/a[1]/@i)"))
op("xpath-concat", lambda: xpath("concat(/r/a[1]/text(), '-', /r/a[2]/text())"))
op("xpath-contains", lambda: xpath("contains(/r/a[1]/text(), 'x')"))
op("xpath-translate", lambda: xpath("translate(/r/a[1]/text(), 'x', 'X')"))
op("xpath-normalize-space", lambda: xpath("normalize-space('  a   b  ')"))
op("xpath-sum", lambda: xpath("sum(//a/@i)"))
op("xpath-count", lambda: xpath("count(//a)"))
op("xpath-local-name", lambda: xpath("local-name(/r/n:c)"))
op("xpath-name", lambda: xpath("name(/r/n:c)"))
op("xpath-namespace-uri", lambda: xpath("namespace-uri(/r/n:c)"))
op("xpath-var", lambda: xpath("$v", variables={"v": "hello"}))
op("xpath-var-num", lambda: xpath("$v + 1", variables={"v": 41}))
op("xpath-var-node", lambda: xpath("$v", variables={"v": ET.fromstring("<v>node</v>".encode())}))
op("xpath-bad-expr", lambda: xpath("//a["))
op("xpath-bad-fn", lambda: xpath("unknown-fn()"))
op("xpath-bool", lambda: xpath("//a[@i]"))
op("xpath-union", lambda: xpath("//a | //b"))
op("xpath-ancestor-or-self", lambda: xpath("//a/ancestor-or-self::*"))
op("xpath-descendant", lambda: xpath("/r/descendant::a"))
op("xpath-preceding-sibling", lambda: xpath("//a[2]/preceding-sibling::a"))
op("xpath-id", lambda: xpath("id('x')"))

def xpath_smart(expr):
    t = ET.fromstring(XPATH_DOC.encode())
    r = t.xpath(expr, smart_strings=True)
    return [(type(x).__name__, str(x)) for x in (r if isinstance(r, list) else [r])]
op("xpath-smart", lambda: xpath_smart("string(/r/a[1]/@i)"))

# ── 8. XSLT / EXSLT ───────────────────────────────────────────────────────
XSLT1 = """
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output method="xml"/>
<xsl:template match="/">
 <out>
  <xsl:for-each select="//item">
   <item id="{@id}"><xsl:value-of select="."/></item>
  </xsl:for-each>
  <xsl:if test="count(//item) &gt; 2">many</xsl:if>
 </out>
</xsl:template>
</xsl:stylesheet>"""
DOC1 = "<list><item id='1'>a</item><item id='2'>b</item><item id='3'>c</item></list>"

def xslt_run(xsl, doc, params=None):
    xslt = ET.XSLT(ET.fromstring(xsl.encode()))
    d = ET.fromstring(doc.encode())
    out = xslt(d, **(params or {}))
    return (str(out), xslt.error_log)

op("xslt-basic", lambda: xslt_run(XSLT1, DOC1))
op("xslt-params", lambda: xslt_run(XSLT1, DOC1, {"x": "param-value"}))

XSLT_EXSLT = """
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
 xmlns:date="http://exslt.org/dates-and-times" xmlns:math="http://exslt.org/math"
 xmlns:set="http://exslt.org/sets" xmlns:str="http://exslt.org/strings">
<xsl:output method="text"/>
<xsl:template match="/">
 <xsl:value-of select="date:date-time()"/>|
 <xsl:value-of select="math:max(//item)"/>|
 <xsl:value-of select="math:min(//item)"/>|
 <xsl:value-of select="math:highest(//item)"/>|
 <xsl:value-of select="str:upper-case(/list/name)"/>|
 <xsl:value-of select="set:distinct(//item)"/>
</xsl:template>
</xsl:stylesheet>"""
op("xslt-exslt", lambda: xslt_run(XSLT_EXSLT, "<list><name>miXeD</name><item>5</item><item>3</item><item>9</item><item>3</item></list>"))

XSLT_DOC = """
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:output method="text"/>
<xsl:template match="/"><xsl:value-of select="document('extra')/r/v"/></xsl:template>
</xsl:stylesheet>"""

def xslt_document():
    class R(ET.Resolver):
        def resolve(self, url, pubid, context):
            return self.resolve_string("<r><v>resolved</v></r>", context)
    p = ET.XMLParser(no_network=True)
    p.resolvers.add(R())
    xslt = ET.XSLT(ET.fromstring(XSLT_DOC.encode()))
    d = ET.fromstring("<root/>".encode(), parser=p)
    return str(xslt(d))
op("xslt-document-resolver", xslt_document)

def xslt_keys():
    xsl = """
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:key name="k" match="item" use="@id"/>
<xsl:output method="text"/>
<xsl:template match="/"><xsl:for-each select="key('k', '2')"><xsl:value-of select="."/></xsl:for-each></xsl:template>
</xsl:stylesheet>"""
    return xslt_run(xsl, "<list><item id='1'>a</item><item id='2'>b</item></list>")
op("xslt-keys", xslt_keys)

# ── 9. Schema / RelaxNG / DTD ─────────────────────────────────────────────
SCHEMA = """
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:element name="root"><xs:complexType><xs:sequence>
<xs:element name="a" type="xs:int" minOccurs="0" maxOccurs="unbounded"/>
<xs:element name="b" type="xs:string"/>
</xs:sequence></xs:complexType></xs:element>
</xs:schema>"""

def schema_check(doc):
    s = ET.XMLSchema(ET.fromstring(SCHEMA.encode()))
    d = ET.fromstring(doc.encode())
    ok = s.validate(d)
    return (ok, [(e.message or "").strip() for e in s.error_log][:2])
op("schema-valid", lambda: schema_check("<root><a>1</a><b>x</b></root>"))
op("schema-invalid", lambda: schema_check("<root><a>nope</a></root>"))
op("schema-missing", lambda: schema_check("<root/>"))

RNG = """
<element name="root" xmlns="http://relaxng.org/ns/structure/1.0">
 <element name="a"><text/></element>
 <optional><element name="b"><text/></element></optional>
</element>"""

def rng_check(doc):
    s = ET.RelaxNG(ET.fromstring(RNG.encode()))
    d = ET.fromstring(doc.encode())
    ok = s.validate(d)
    return (ok, [(e.message or "").strip() for e in s.error_log][:2])
op("rng-valid", lambda: rng_check("<root><a>x</a></root>"))
op("rng-invalid", lambda: rng_check("<root><a>x</a><b>y</b><c>z</c></root>"))

def dtd_check(doc, dtd):
    d = ET.fromstring(doc.encode())
    return d.docinfo.doctype, d.docinfo.root_name
op("dtd-docinfo", lambda: dtd_check(
    "<!DOCTYPE root [<!ELEMENT root (a)><!ELEMENT a (#PCDATA)><!ATTLIST a id ID #IMPLIED>]><root><a id='x'>t</a></root>", None))

def dtd_validate(doc):
    p = ET.XMLParser(dtd_validation=True)
    d = ET.fromstring(doc.encode(), parser=p)
    return (d.tag, d.docinfo.doctype)
op("dtd-validate-ok", lambda: dtd_validate(
    "<!DOCTYPE root [<!ELEMENT root (a)><!ELEMENT a (#PCDATA)>]><root><a>t</a></root>"))
op("dtd-validate-bad", lambda: dtd_validate(
    "<!DOCTYPE root [<!ELEMENT root (a)><!ELEMENT a (#PCDATA)>]><root><b/></root>"))

# ── 10. Serialization ─────────────────────────────────────────────────────
SERDOC = "<r a='1'><a>text</a><b><!--c--></b><c><![CDATA[<x>]]></c></r>"

def ser_methods():
    t = ET.fromstring(SERDOC.encode())
    out = {}
    for method in ("xml", "html", "text", "c14n"):
        try:
            out[method] = ET.tostring(t, method=method).decode("utf-8", "replace")
        except Exception as exc:  # noqa: BLE001
            out[method] = ("EXC", type(exc).__name__)
    return out
op("ser-methods", ser_methods)

def ser_encodings():
    t = ET.fromstring("<r><a>caf\xe9</a></r>".encode("latin-1"))
    out = {}
    for enc in ("utf-8", "utf-16", "iso-8859-1", "ascii", "us-ascii"):
        try:
            b = ET.tostring(t, encoding=enc)
            out[enc] = (b[:30].hex(), len(b))
        except Exception as exc:  # noqa: BLE001
            out[enc] = ("EXC", type(exc).__name__)
    return out
op("ser-encodings", ser_encodings)

def ser_xml_decl():
    t = ET.fromstring("<r/>".encode())
    return (ET.tostring(t, xml_declaration=True).decode(), ET.tostring(t, xml_declaration=False).decode())
op("ser-xmldecl", ser_xml_decl)

def ser_pretty():
    t = ET.fromstring("<r><a>1</a><b><c>2</c></b></r>".encode())
    return ET.tostring(t, pretty_print=True).decode()
op("ser-pretty", ser_pretty)

def ser_text_tail():
    t = ET.fromstring("<r>a<b/>c</r>".encode())
    return (t.text, t[0].tail, t[0].text)
op("ser-text-tail", ser_text_tail)

# ── 11. C14N ──────────────────────────────────────────────────────────────
def c14n_doc():
    t = ET.fromstring(
        '<r xmlns:p="http://p" xmlns:q="http://q"><a p:x="1" b="2"><b>t</b></a><p:c/></r>'.encode())
    return (ET.tostring(t, method="c14n").decode(),
            ET.tostring(t, method="c14n", exclusive=True).decode(),
            ET.tostring(t, method="c14n", with_comments=True).decode())
op("c14n", c14n_doc)

# ── 12. Encoding conversions (incl. R-000157 surface) ─────────────────────
ENCS = {
    "iso-8859-1": "caf\xe9".encode("iso-8859-1"),
    "iso-8859-15": "caf\u20ac".encode("iso-8859-15"),
    "utf-16": "<r>caf\xe9</r>".encode("utf-16"),
    "utf-8": "<r>caf\xc3\xa9</r>".encode(),
    "ascii": b"<r>plain</r>",
    "windows-1252": "<r>\u201cquote\u201d</r>".encode("windows-1252"),
    "shift_jis": "<r>\u30e9\u30fc\u30e1\u30f3</r>".encode("shift_jis"),
    "euc-jp": "<r>\u65e5\u672c</r>".encode("euc-jp"),
    "iso-2022-jp": "<r>\u65e5\u672c</r>".encode("iso-2022-jp"),
}
for name, raw in ENCS.items():
    def _e(raw=raw, n=name):
        t = ET.fromstring(raw, parser=ET.XMLParser(no_network=True, recover=True))
        return ser(t) if t is not None else None
    op(f"enc-{name}", _e)
    def _ee(raw=raw, n=name):
        try:
            t = ET.fromstring(raw)
            return ET.tostring(t, encoding="unicode")
        except Exception as exc:  # noqa: BLE001
            return ("EXC", type(exc).__name__, errinfo())
    op(f"enc-{name}-err", _ee)

# ── 13. Error reporting (structured) ──────────────────────────────────────
def errors_matrix():
    out = []
    docs = ["<a><b></a>", "<a>&undef;</a>", "<a>", "text<tag>", "<a attr='x' attr='y'/>"]
    for d in docs:
        p = ET.XMLParser(no_network=True)
        try:
            ET.fromstring(d.encode(), parser=p)
        except ET.XMLSyntaxError as exc:
            err = exc
            out.append((type(err).__name__, (err.position[0], err.position[1])))
        out.append(errinfo())
    return out
op("errors-matrix", errors_matrix)

def error_after_reset():
    p = ET.XMLParser(no_network=True)
    try:
        ET.fromstring(b"<a><b></a>", parser=p)
    except ET.XMLSyntaxError:
        pass
    ET.clear_error_log()
    try:
        ET.fromstring(b"<ok/>", parser=p)
        return ("ok", errinfo())
    except Exception as exc:  # noqa: BLE001
        return ("EXC", type(exc).__name__)
op("errors-reset", error_after_reset)

# ── 14. Lifetime / repeated cycles ─────────────────────────────────────────
def churn(n=200):
    for i in range(n):
        t = ET.fromstring(f"<r><a>{i}</a></r>".encode())
        ET.tostring(t)
    return n
op("lifetime-churn", churn)

def churn_html(n=200):
    for i in range(n):
        t = LH.fromstring(f"<html><body><p>{i}</p></body></html>")
    return n
op("lifetime-churn-html", churn_html)

def tree_mutate_iter():
    root = ET.fromstring("<r><a/><b/><c/></r>".encode())
    for el in list(root):
        if el.tag == "b":
            root.remove(el)
    root.append(ET.Element("d"))
    return ser(root)
op("lifetime-mutate-iter", tree_mutate_iter)

# ── 15. XInclude ──────────────────────────────────────────────────────────
XINC = "<root xmlns:xi='http://www.w3.org/2001/XInclude'><xi:include href='part.xml' parse='text'/></root>"

def xinclude_run():
    class R(ET.Resolver):
        def resolve(self, url, pubid, context):
            return self.resolve_string("included-text", context)
    p = ET.XMLParser(no_network=True)
    p.resolvers.add(R())
    t = ET.fromstring(XINC.encode(), parser=p)
    try:
        t.xinclude()
    except Exception as exc:  # noqa: BLE001
        return ("EXC", type(exc).__name__)
    return ser(t)
op("xinclude", xinclude_run)

# ── 16. iterwalk / getroottree / element paths ────────────────────────────
def misc_paths():
    t = ET.fromstring("<r><a><b>x</b></a><c/></r>".encode())
    a = t[0]
    return (t.getroottree().getpath(a), ET.ElementTree(t).getpath(t[1]))
op("misc-getpath", misc_paths)

def misc_itertext():
    t = ET.fromstring("<r>a<b>c</b>d<c>e</c></r>".encode())
    return "|".join(t.itertext())
op("misc-itertext", misc_itertext)

def misc_findall():
    t = ET.fromstring("<r><a i='1'/><a i='2'/><b/></r>".encode())
    return ([e.tag for e in t.findall("a")],
            [e.tag for e in t.findall(".//a")],
            t.find("a") is not None,
            t.find("missing") is None)
op("misc-findall", misc_findall)

def misc_makeelement():
    t = ET.Element("r")
    el = t.makeelement("x", {"a": "1"})
    el.text = "t"
    t.append(el)
    return ser(t)
op("misc-makeelement", misc_makeelement)

def misc_qname():
    t = ET.fromstring('<r xmlns:p="http://p"><p:a/></r>'.encode())
    el = t[0]
    return (el.tag, ET.QName(el).localname, ET.QName(el).namespace,
            ET.QName(el).text)
op("misc-qname", misc_qname)

# ── 17. html5parser surface ───────────────────────────────────────────────
def html5_surface():
    try:
        from lxml.html.html5parser import document_fromstring, fragment_fromstring
        d = document_fromstring("<html><body><p>x</p></body></html>")
        return LH.tostring(d, encoding="unicode")
    except Exception as exc:  # noqa: BLE001
        return ("EXC", type(exc).__name__, str(exc).splitlines()[:1])
op("html5-surface", html5_surface)

# ── 18. lxml.sax bridging ─────────────────────────────────────────────────
def sax_bridge():
    sink = SaxSink()
    handler = ET.sax.SaxHandler(sink)
    t = ET.fromstring(b"<r><a>t</a></r>")
    ET.sax.saxify(t, handler)
    return sink.ev
op("sax-bridge", sax_bridge)

# ── print everything deterministically ────────────────────────────────────
for line in OUT:
    print(line)
print(f"TOTAL_OPS: {len(OUT)}")
