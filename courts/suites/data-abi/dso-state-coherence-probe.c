/*
 * DSO-STATE-COHERENCE-001 — cross-DSO shared-state probe (11.1-Z.2).
 *
 * Linked against BOTH -lxslt and -lxml2 (separate DSOs). The probe installs
 * process-visible state through the libxml2 DSO and then runs an XSLT
 * transform whose implementation lives in the libxslt DSO:
 *
 *   Phase P1 (compile) — xsltParseStylesheetDoc:
 *     custom allocator / node register / node deregister hooks must observe
 *     the stylesheet compilation.
 *   Phase P2 (transform) — xsltApplyStylesheet with document('aux.xml'):
 *     the same hooks plus the external entity loader must observe the
 *     transform; and the libxml2 keepBlanks global (set to 0 through the
 *     libxml2 DSO) must govern the parse libxslt performs for document() —
 *     the string-length of the loaded aux element is reported.
 *
 * For a shared-instance libxml2+libxslt (the oracle) every observation is
 * TRUE and the reported length reflects keepBlanks=0. A whole-archive
 * facade libxslt that carries a private copy of the libxml2 core and its
 * statics observes NONE of the hooks and reports a different length — the
 * court FAILs, demonstrating state partitioning.
 *
 * The probe prints booleans (never addresses or counts that depend on
 * implementation allocation patterns) plus the deterministic length value.
 * Run via tools/abi/dso_state_coherence_probe.py.
 */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlmemory.h>
#include <libxml/xmlIO.h>
#include <libxml/xmlsave.h>
#include <libxslt/xslt.h>
#include <libxslt/transform.h>

static int my_alloc = 0;
static void *my_malloc(size_t s) { my_alloc++; return malloc(s); }
static void my_free(void *p) { free(p); }
static void *my_realloc(void *p, size_t s) { return realloc(p, s); }
static char *my_strdup(const char *s) { return strdup(s); }

static int reg_seen = 0, dereg_seen = 0;
static void my_reg(xmlNodePtr n) { (void) n; reg_seen++; }
static void my_dereg(xmlNodePtr n) { (void) n; dereg_seen++; }

static int loader_seen = 0;
static xmlParserInputPtr my_loader(const char *URL, const char *ID,
                                   xmlParserCtxtPtr ctxt) {
    (void) ID; (void) ctxt;
    loader_seen++;
    return xmlNoNetExternalEntityLoader(URL, ID, ctxt);
}

static const char XSL[] =
    "<?xml version=\"1.0\"?>\n"
    "<xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\n"
    "  <xsl:template match=\"/\">\n"
    "    <out>\n"
    "      <xsl:value-of select=\"root/name\"/>\n"
    "      <xsl:value-of select=\"string-length(document('aux.xml')/aux)\"/>\n"
    "    </out>\n"
    "  </xsl:template>\n"
    "</xsl:stylesheet>\n";

static const char DOC[] =
    "<?xml version=\"1.0\"?>\n"
    "<!DOCTYPE root [<!ENTITY ext SYSTEM \"ext.xml\">]>\n"
    "<root><name>hello</name><extra>&ext;</extra></root>\n";

static const char EXT[] = "<extra-data/>\n";
static const char AUX[] =
    "<?xml version=\"1.0\"?>\n"
    "<!DOCTYPE aux [<!ENTITY e SYSTEM \"e.xml\">]>\n"
    "<aux>\n  <child/>\n  &e;\n</aux>\n";
static const char EXML[] = "ENTITYDATA";

int main(void) {
    int rc;
    FILE *fp;

    /* Install state through the libxml2 DSO. */
    rc = xmlMemSetup(my_free, my_malloc, my_realloc, my_strdup);
    if (rc != 0) { printf("alloc-install-rc=%d\n", rc); return 1; }
    xmlRegisterNodeDefault(my_reg);
    xmlDeregisterNodeDefault(my_dereg);
    xmlSetExternalEntityLoader(my_loader);
    xmlKeepBlanksDefault(0);

    /* Auxiliary files for the document() load inside the transform. */
    fp = fopen("aux.xml", "w"); if (fp) { fputs(AUX, fp); fclose(fp); }
    fp = fopen("e.xml", "w"); if (fp) { fputs(EXML, fp); fclose(fp); }

    /* Phase 0 — entity loader through libxml2 (shared-instance sanity). */
    loader_seen = 0;
    {
        xmlDocPtr d = xmlReadMemory(DOC, (int) strlen(DOC), "doc.xml", NULL,
                                    XML_PARSE_NOENT);
        if (d != NULL) xmlFreeDoc(d);
    }
    printf("loader_observed_main_parse=%d\n", loader_seen > 0);

    /* Phase P1 — stylesheet compile inside the libxslt DSO. The source
     * stylesheet parse happens BEFORE the window so only libxslt-internal
     * work is measured. */
    {
        xmlDocPtr sdoc = xmlReadMemory(XSL, (int) strlen(XSL), "style.xsl",
                                       NULL, 0);
        my_alloc = reg_seen = dereg_seen = 0;
        xsltStylesheetPtr ss = xsltParseStylesheetDoc(sdoc);
        if (ss == NULL) {
            printf("style-parse-failed\n");
            return 1;
        }
        printf("p1_allocator_observed=%d\n", my_alloc > 0);
        printf("p1_reg_observed=%d\n", reg_seen > 0);
        printf("p1_dereg_observed=%d\n", dereg_seen > 0);

        /* Phase P2 — transform inside the libxslt DSO; the source parse
         * again happens before the window. */
        {
            xmlDocPtr doc = xmlReadMemory(DOC, (int) strlen(DOC), "doc.xml",
                                          NULL, XML_PARSE_NOENT);
            my_alloc = reg_seen = dereg_seen = loader_seen = 0;
            xmlDocPtr res = xsltApplyStylesheet(ss, doc, NULL);
            printf("p2_allocator_observed=%d\n", my_alloc > 0);
            printf("p2_reg_observed=%d\n", reg_seen > 0);
            printf("p2_dereg_observed=%d\n", dereg_seen > 0);
            printf("p2_loader_observed=%d\n", loader_seen > 0);
            if (res != NULL) {
                xmlChar *mem = NULL;
                int size = 0;
                xmlDocDumpFormatMemory(res, &mem, &size, 1);
                if (mem != NULL) {
                    /* report the deterministic size of the result (the
                     * document() whitespace-stripping difference shows up
                     * here when the keepBlanks global is partitioned) */
                    const char *m = (const char *) mem;
                    printf("p2_result_size=%d\n", size);
                    printf("p2_result_has_entity=%d\n",
                           strstr(m, "ENTITYDATA") != NULL);
                    xmlFree(mem);
                }
                xmlFreeDoc(res);
            }
            xmlFreeDoc(doc);
        }
        xsltFreeStylesheet(ss);
    }

    remove("aux.xml");
    remove("e.xml");
    return 0;
}
