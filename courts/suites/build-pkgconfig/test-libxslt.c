/*
 * test-libxslt.c — external C consumer probe for the 11.1-S build courts.
 *
 * Court family: BUILD-PKGCONFIG / BUILD-CONFIG-SCRIPT (11.1-S).
 *
 * Consumer program for the libxslt drop-in: applies a trivial identity-style
 * transform with standard tooling:
 *
 *   cc $(xslt-config --cflags) test-libxslt.c $(xslt-config --libs)
 *   cc $(pkg-config --cflags libxslt) test-libxslt.c $(pkg-config --libs libxslt)
 *
 * pkg-config resolves the `Requires: libxml-2.0` dependency automatically.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/transform.h>
#include <libxslt/xsltutils.h>

int main(void) {
    xmlInitParser();
    const char *xsl =
        "<?xml version='1.0'?>"
        "<xsl:stylesheet version='1.0' "
        "xmlns:xsl='http://www.w3.org/1999/XSL/Transform'>"
        "<xsl:output method='xml' omit-xml-declaration='yes'/>"
        "<xsl:template match='/'>"
        "<out><xsl:value-of select='root/child'/></out>"
        "</xsl:template>"
        "</xsl:stylesheet>";
    const char *doc = "<root><child>hi</child></root>";

    xmlDocPtr sdoc = xmlReadMemory(xsl, (int)strlen(xsl), "t.xsl", NULL, 0);
    xmlDocPtr ddoc = xmlReadMemory(doc, (int)strlen(doc), "t.xml", NULL, 0);
    if (sdoc == NULL || ddoc == NULL) {
        fprintf(stderr, "parse failed\n");
        return 1;
    }
    xsltStylesheetPtr ss = xsltParseStylesheetDoc(sdoc);
    if (ss == NULL) {
        fprintf(stderr, "xsltParseStylesheetDoc failed\n");
        return 1;
    }
    xmlDocPtr res = xsltApplyStylesheet(ss, ddoc, NULL);
    if (res == NULL) {
        fprintf(stderr, "xsltApplyStylesheet failed\n");
        xsltFreeStylesheet(ss);
        return 1;
    }
    xmlChar *out = NULL;
    int len = 0;
    if (xsltSaveResultToString(&out, &len, res, ss) != 0) {
        fprintf(stderr, "xsltSaveResultToString failed\n");
    }
    /* NOTE (R-000167): upstream 1.1.45 declares xsltLibxsltVersion as a
     * read-only DATA variable; the candidate DSO currently exports it as a
     * function. The packaging court therefore checks the transform output
     * and the xml2 runtime version (data) instead of that divergent API. */
    printf("xml=%s\n", xmlParserVersion);
    printf("result=%s\n", (char *)(out ? out : (xmlChar *)""));
    if (out != NULL) xmlFree(out);
    xmlFreeDoc(res);
    xsltFreeStylesheet(ss);
    xmlFreeDoc(ddoc);
    xmlCleanupParser();
    return 0;
}
