/*
 * elf-versioning-consumer.c — Phase 12 ELF-VERSIONING / BINARY-SUBSTITUTION
 * probe. A REAL consumer compiled against the ORACLE headers/DSOs, then run
 * unmodified against the candidate DSOs (LD_LIBRARY_PATH substitution).
 *
 * Court family: ELF-VERSIONING / BINARY-SUBSTITUTION
 *
 *   - -lxml2 parse + version string
 *   - -lxslt compile + transform + save
 *   - -lexslt register + version ints
 *   - dlvsym() against named version nodes (LIBXML2_1.x on libxslt): the
 *     exact nodes must resolve on the candidate, bogus nodes must fail.
 *
 * Output is deterministic so the oracle-run and candidate-run can be
 * compared byte-for-byte.
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

    /* -lxml2 surface */
    const char *DOC = "<root n='7'><a>1</a></root>";
    xmlDocPtr d = xmlReadMemory(DOC, (int)strlen(DOC), "t.xml", NULL, 0);
    printf("xml doc=%d version=%s\n", d != NULL, xmlParserVersion);
    if (!d) return 1;

    /* -lxslt surface (compile + transform + save) */
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

    /* version data (R-000167 family): xsltLibxsltVersion/xsltLibxmlVersion
     * are const int; xsltEngineVersion is const char * */
    printf("xslt ver=%d %s %d\n", xsltLibxsltVersion, xsltEngineVersion,
           xsltLibxmlVersion);

    /* -lexslt surface */
    exsltRegisterAll();
    printf("exslt ver=%d %d %d %s\n", exsltLibexsltVersion, exsltLibxmlVersion,
           exsltLibxsltVersion, exsltLibraryVersion);

    /* dlvsym: the oracle's named LIBXML2_1.x nodes must resolve on the
     * substituted candidate; a bogus node must NOT. */
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
