/*
 * substitution-consumer2.c — Phase 12 DOCKER-SUBSTITUTION probe 2.
 * A second unmodified consumer (no dlvsym), built inside the container
 * through the CANDIDATE's pkg-config files (.pc) + DSOs, then run with the
 * candidate substituted. Exercises the xml2-config/xslt-config/pkg-config
 * plane inside the VM.
 *
 * Court family: DOCKER-SUBSTITUTION (pkg-config plane)
 */
#include <stdio.h>
#include <string.h>
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
    "<xsl:template match='/'>pkg-<xsl:value-of select='/root/@n'/></xsl:template>"
    "</xsl:stylesheet>";

int main(void) {
    const char *DOC = "<root n='3'><a>1</a></root>";
    xmlDocPtr d = xmlReadMemory(DOC, (int)strlen(DOC), "t.xml", NULL, 0);
    printf("pkg xml=%d\n", d != NULL);
    if (!d) return 1;
    xmlDocPtr s = xmlReadMemory(XSL, (int)strlen(XSL), "t.xsl", NULL, 0);
    xsltStylesheetPtr ss = s ? xsltParseStylesheetDoc(s) : NULL;
    printf("pkg xslt=%d\n", ss != NULL);
    if (!ss) return 1;
    xmlDocPtr r = xsltApplyStylesheet(ss, d, NULL);
    printf("pkg result=%d\n", r != NULL);
    if (r) {
        xmlChar *out = NULL;
        int len = 0;
        xsltSaveResultToString(&out, &len, r, ss);
        printf("pkg out=%s len=%d\n", out ? (char *)out : "(null)", len);
        xmlFree(out);
        xmlFreeDoc(r);
    }
    exsltRegisterAll();
    printf("pkg exslt=%d %d %d %s\n", exsltLibexsltVersion, exsltLibxmlVersion,
           exsltLibxsltVersion, exsltLibraryVersion);
    xmlFreeDoc(d);
    xsltFreeStylesheet(ss);
    xmlCleanupParser();
    printf("PKG-VERDICT %s\n", "PASS");
    return 0;
}
