/*
 * WRITER-001 — differential probe of the xmlTextWriter* family
 * (11.1-I writer closure).
 *
 * Compiled twice (oracle DSO vs candidate DSO); output must be byte-identical.
 * Prints both the produced XML and the return values (the upstream writer
 * returns the number of bytes written per call).
 */
#include <stdio.h>
#include <string.h>
#include <stdarg.h>
#include <libxml/xmlwriter.h>
#include <libxml/tree.h>

/* Forward a variadic call to the VFormat entry point (legal va_start site). */
static int vfmt_string_fwd(xmlTextWriterPtr w, const char *fmt, ...) {
    va_list ap;
    int r;
    va_start(ap, fmt);
    r = (int) xmlTextWriterWriteVFormatString(w, fmt, ap);
    va_end(ap);
    return r;
}

static void dump(xmlTextWriterPtr w, xmlBufferPtr buf) {
    xmlTextWriterFlush(w);
    printf("---\n%.*s\n", (int) xmlBufferLength(buf), (const char *) xmlBufferContent(buf));
}

int main(void) {
    xmlBufferPtr buf = xmlBufferCreate();
    xmlTextWriterPtr w = xmlNewTextWriterMemory(buf, 0);
    int r;

    /* 1. basic document, indented */
    r = (int) xmlTextWriterSetIndent(w, 1);
    printf("setindent=%d\n", r);
    r = (int) xmlTextWriterStartDocument(w, NULL, "UTF-8", NULL);
    printf("startdoc=%d\n", r);
    r = (int) xmlTextWriterStartElement(w, (const xmlChar *) "root");
    printf("start-root=%d\n", r);
    r = (int) xmlTextWriterStartElement(w, (const xmlChar *) "child");
    printf("start-child=%d\n", r);
    r = (int) xmlTextWriterWriteAttribute(w, (const xmlChar *) "a", (const xmlChar *) "1 & 2 < \"q\"");
    printf("attr=%d\n", r);
    r = (int) xmlTextWriterWriteString(w, (const xmlChar *) "text <>&\"'");
    printf("string=%d\n", r);
    r = (int) xmlTextWriterEndElement(w);
    printf("end-child=%d\n", r);
    r = (int) xmlTextWriterWriteComment(w, (const xmlChar *) "a comment");
    printf("comment=%d\n", r);
    r = (int) xmlTextWriterWritePI(w, (const xmlChar *) "target", (const xmlChar *) "data");
    printf("pi=%d\n", r);
    r = (int) xmlTextWriterWriteCDATA(w, (const xmlChar *) "cdata <&>");
    printf("cdata=%d\n", r);
    r = (int) xmlTextWriterFullEndElement(w);
    printf("full-end=%d\n", r);
    r = (int) xmlTextWriterEndDocument(w);
    printf("enddoc=%d\n", r);
    dump(w, buf);

    /* 2. namespaced element + attribute */
    r = (int) xmlTextWriterStartElementNS(w, (const xmlChar *) "p", (const xmlChar *) "node",
                                          (const xmlChar *) "urn:test");
    printf("ns-start=%d\n", r);
    r = (int) xmlTextWriterWriteAttributeNS(w, (const xmlChar *) "p", (const xmlChar *) "attr",
                                            (const xmlChar *) "urn:test",
                                            (const xmlChar *) "v");
    printf("ns-attr=%d\n", r);
    r = (int) xmlTextWriterEndElement(w);
    printf("ns-end=%d\n", r);
    r = (int) xmlTextWriterEndDocument(w);
    dump(w, buf);

    /* 3. DTD family with a parameter entity and external entities */
    r = (int) xmlTextWriterStartDTD(w, (const xmlChar *) "root", NULL, NULL);
    printf("startdtd=%d\n", r);
    r = (int) xmlTextWriterWriteDTDElement(w, (const xmlChar *) "child", (const xmlChar *) "(#PCDATA|sub)*");
    printf("dtdelem=%d\n", r);
    r = (int) xmlTextWriterWriteDTDAttlist(w, (const xmlChar *) "child",
                                           (const xmlChar *) "id CDATA #IMPLIED");
    printf("dtdattlist=%d\n", r);
    r = (int) xmlTextWriterWriteDTDInternalEntity(w, 1, (const xmlChar *) "pent",
                                                  (const xmlChar *) "pe-content");
    printf("dtdpent=%d\n", r);
    r = (int) xmlTextWriterWriteDTDInternalEntity(w, 0, (const xmlChar *) "ent",
                                                  (const xmlChar *) "content &amp; more");
    printf("dtdent=%d\n", r);
    r = (int) xmlTextWriterWriteDTDExternalEntity(w, 0, (const xmlChar *) "ext",
                                                  (const xmlChar *) "-//PUB//ID",
                                                  (const xmlChar *) "http://x/y.dtd",
                                                  NULL);
    printf("dtdext=%d\n", r);
    r = (int) xmlTextWriterWriteDTDNotation(w, (const xmlChar *) "note",
                                            (const xmlChar *) "PubID", (const xmlChar *) "SysID");
    printf("dtdnot=%d\n", r);
    r = (int) xmlTextWriterEndDTD(w);
    printf("enddtd=%d\n", r);
    r = (int) xmlTextWriterStartElement(w, (const xmlChar *) "root");
    r = (int) xmlTextWriterEndElement(w);
    r = (int) xmlTextWriterEndDocument(w);
    dump(w, buf);

    /* 4. quote char + non-indented + empty elements */
    r = (int) xmlTextWriterSetQuoteChar(w, '\'');
    printf("setquote=%d\n", r);
    r = (int) xmlTextWriterSetIndent(w, 0);
    r = (int) xmlTextWriterStartDocument(w, NULL, NULL, "yes");
    printf("startdoc2=%d\n", r);
    r = (int) xmlTextWriterStartElement(w, (const xmlChar *) "a");
    r = (int) xmlTextWriterWriteAttribute(w, (const xmlChar *) "q", (const xmlChar *) "it's 'quoted'");
    printf("attr-squote=%d\n", r);
    r = (int) xmlTextWriterStartElement(w, (const xmlChar *) "empty");
    r = (int) xmlTextWriterEndElement(w);
    printf("empty-end=%d\n", r);
    r = (int) xmlTextWriterStartElement(w, (const xmlChar *) "b");
    r = (int) xmlTextWriterWriteString(w, (const xmlChar *) "tail");
    r = (int) xmlTextWriterEndElement(w);
    r = (int) xmlTextWriterEndElement(w);
    r = (int) xmlTextWriterEndDocument(w);
    dump(w, buf);

    /* 5. Format / VFormat variadic family (incl. >6 GP overflow args) */
    r = (int) xmlTextWriterStartDocument(w, NULL, NULL, NULL);
    r = (int) xmlTextWriterStartElement(w, (const xmlChar *) "fmt");
    r = (int) xmlTextWriterWriteFormatString(w, "str %s %d %x %c %ld", "s", 42, 255, 'Z', 1234567890L);
    printf("fmt-string=%d\n", r);
    r = (int) xmlTextWriterWriteFormatAttribute(w, "count", "%d", 7);
    printf("fmt-attr=%d\n", r);
    r = (int) xmlTextWriterWriteFormatElement(w, "e", "%s=%d", "x", 1);
    printf("fmt-elem=%d\n", r);
    r = (int) xmlTextWriterWriteFormatComment(w, "c %d", 99);
    printf("fmt-comment=%d\n", r);
    r = (int) xmlTextWriterWriteFormatPI(w, "p", "v=%s", "z");
    printf("fmt-pi=%d\n", r);
    r = (int) xmlTextWriterWriteFormatCDATA(w, "cd %d", 5);
    printf("fmt-cdata=%d\n", r);
    /* overflow path: 7 GP varargs (only 5 fit in registers) */
    r = (int) xmlTextWriterWriteFormatRaw(w, "%d %d %d %d %d %d %d", 1, 2, 3, 4, 5, 6, 7);
    printf("fmt-raw-overflow=%d\n", r);
    /* VFormat direct call */
    r = vfmt_string_fwd(w, "%s|%d", "v", 3);
    printf("vfmt-string=%d\n", r);
    r = (int) xmlTextWriterEndElement(w);
    r = (int) xmlTextWriterEndDocument(w);
    dump(w, buf);

    /* 6. NULL/error paths */
    printf("null-writer=%d\n", (int) xmlTextWriterWriteString(NULL, (const xmlChar *) "x"));
    printf("null-content=%d\n", (int) xmlTextWriterWriteString(w, NULL));
    printf("close-null=%d\n", (int) xmlTextWriterClose(NULL));
    printf("setquote-bad=%d\n", (int) xmlTextWriterSetQuoteChar(w, 'x'));
    r = (int) xmlTextWriterClose(w);
    printf("close=%d\n", r);
    printf("close-again=%d\n", (int) xmlTextWriterClose(w));
    xmlFreeTextWriter(w);
    xmlBufferFree(buf);
    return 0;
}
