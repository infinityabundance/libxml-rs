/*
 * substitution-consumer.c — Phase 12 DOCKER-SUBSTITUTION probe 1.
 * A REAL consumer compiled inside the container against the canonical oracle
 * (libxml2 2.15.3 + libxslt 1.1.45 + libexslt 0.8.25 in /usr/local), then
 * run unmodified against the candidate DSOs (LD_LIBRARY_PATH=/candidate).
 *
 * Court family: DOCKER-SUBSTITUTION
 *
 *   - -lxml2 parse + version string
 *   - -lxslt compile + transform + save
 *   - -lexslt register + version ints
 *   - dlvsym() against named LIBXML2_1.x nodes on libxslt
 *
 * Output is deterministic so the oracle-run and candidate-run can be
 * compared byte-for-byte (the container oracle is the same 2.15.3 epoch as
 * the candidate, so even xmlParserVersion matches).
 */
#define _GNU_SOURCE
#include <stdio.h>
#include <string.h>
#include <dlfcn.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/transform.h>
#include <libxslt/xsltutils.h>
#include <libexslt/exslt.h>

static const char XSL[] =
    "<?xml version='1.0'?>"
    "<xsl:stylesheet version='1.0'"
    " xmlns:xsl='http://www.w3.org/1999/XSL/Transform'>"
    "<xsl:template match='/'>hello-<xsl:value-of select='/root/@n'/></xsl:template>"
    "</xsl:stylesheet>";

int main(void) {
    int rc = 0;

    const char *DOC = "<root n='7'><a>1</a></root>";
    xmlDocPtr d = xmlReadMemory(DOC, (int)strlen(DOC), "t.xml", NULL, 0);
    printf("xml doc=%d version=%s\n", d != NULL, xmlParserVersion);
    if (!d) return 1;

    xmlDocPtr s = xmlReadMemory(XSL, (int)strlen(XSL), "t.xsl", NULL, 0);
    xsltStylesheetPtr ss = s ? xsltParseStylesheetDoc(s) : NULL;
    printf("xslt compile=%d\n", ss != NULL);
    if (!ss) return 1;
    xmlDocPtr r = xsltApplyStylesheet(ss, d, NULL);
    printf("xslt result=%d\n", r != NULL);
    if (r) {
        xmlChar *out = NULL;
        int len = 0;
        xsltSaveResultToString(&out, &len, r, ss);
        printf("xslt out=%s len=%d\n", out ? (char *)out : "(null)", len);
        xmlFree(out);
        xmlFreeDoc(r);
    }
    printf("xslt ver=%d %s %d\n", xsltLibxsltVersion, xsltEngineVersion,
           xsltLibxmlVersion);

    exsltRegisterAll();
    printf("exslt ver=%d %d %d %s\n", exsltLibexsltVersion, exsltLibxmlVersion,
           exsltLibxsltVersion, exsltLibraryVersion);

    void *h = dlopen("libxslt.so.1", RTLD_NOW | RTLD_LOCAL);
    if (!h) { printf("dlopen fail\n"); return 1; }
    void *p1 = dlvsym(h, "xsltAddKey", "LIBXML2_1.0.11");
    void *p2 = dlvsym(h, "xsltAllocateExtra", "LIBXML2_1.0.12");
    void *p3 = dlvsym(h, "xsltParseStylesheetUser", "LIBXML2_1.1.34");
    void *p4 = dlvsym(h, "xsltAddKey", "LIBXML2_1.1.34");
    void *pbad = dlvsym(h, "xsltAddKey", "LIBXML2_9.9.9");
    printf("dlvsym node110=%d node112=%d node1134=%d wrong-node=%d bogus=%d\n",
           p1 != NULL, p2 != NULL, p3 != NULL, p4 != NULL, pbad == NULL);
    dlclose(h);

    xmlFreeDoc(d);
    xsltFreeStylesheet(ss);
    xmlCleanupParser();
    printf("VERDICT %s\n", rc ? "FAIL" : "PASS");
    return rc;
}
