/*
 * hostile-failure-probe.c — Phase 13 HOSTILE-FAILURE attack court.
 *
 * Drives the defined failure paths of the library with inputs that must
 * fail deterministically and identically on both sides:
 *
 *   F1. excessive nesting depth (no XML_PARSE_HUGE);
 *   F2. entity reference loop;
 *   F3. XPath compile failures (NULL, empty, malformed expressions);
 *   F4. XPath eval on a broken/empty document;
 *   F5. save failures (unwritable path, NULL arguments);
 *   F6. xmlNodeDump on a detached node with extreme format flags;
 *   F7. DTD parse failures (NULL ids, missing system id file);
 *   F8. regexp compile failures (malformed patterns);
 *   F9. reader failure paths (garbage input, read past end);
 *   F10. resource-limit: amplification guard trip on a small document.
 *
 * Every failure is DEFINED on both sides; stdout and stderr are compared
 * byte-for-byte (including the diagnostic text and caret windows).
 *
 * Court family: HOSTILE-FAILURE (Phase 13 hostile audit, dimension 5:
 * failure paths)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>
#include <libxml/xmlsave.h>
#include <libxml/xmlreader.h>
#include <libxml/xmlregexp.h>
#include <libxml/xmlIO.h>

/* build a document with `depth` nested elements */
static char *deep_doc(int depth) {
    /* each level: "<aN>" <= 6 bytes + "</aN>" <= 7 bytes + 1 'x' + NUL */
    size_t n = (size_t)depth * 14 + 8;
    char *s = malloc(n);
    if (s == NULL)
        return NULL;
    size_t p = 0;
    for (int i = 0; i < depth; i++)
        p += (size_t)sprintf(s + p, "<a%d>", i);
    p += (size_t)sprintf(s + p, "x");
    for (int i = depth - 1; i >= 0; i--)
        p += (size_t)sprintf(s + p, "</a%d>", i);
    return s;
}

int main(void) {
    LIBXML_TEST_VERSION

    /* ── F1. excessive nesting depth ───────────────────────────────────── */
    {
        char *doc = deep_doc(300);
        xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "t", NULL, 0);
        printf("F1 depth300: doc=%s\n", d ? "(ptr)" : "(nil)");
        if (d) xmlFreeDoc(d);
        free(doc);
    }

    /* ── F2. entity reference loop ─────────────────────────────────────── */
    {
        const char *doc = "<!DOCTYPE r [<!ENTITY a \"&a;\">]><r>&a;</r>";
        xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "t", NULL,
                                    XML_PARSE_NOENT);
        printf("F2 loop: doc=%s\n", d ? "(ptr)" : "(nil)");
        if (d) xmlFreeDoc(d);
    }

    /* ── F3. XPath compile failures ────────────────────────────────────── */
    printf("F3 compile NULL=%s\n", (void *)xmlXPathCompile(NULL) ? "(ptr)" : "(nil)");
    printf("F3 compile empty=%s\n", (void *)xmlXPathCompile(BAD_CAST "") ? "(ptr)" : "(nil)");
    printf("F3 compile garbage=%s\n",
           (void *)xmlXPathCompile(BAD_CAST "//[unclosed") ? "(ptr)" : "(nil)");
    printf("F3 compile badop=%s\n",
           (void *)xmlXPathCompile(BAD_CAST "1 + +") ? "(ptr)" : "(nil)");

    /* ── F4. XPath eval failures ───────────────────────────────────────── */
    {
        xmlDocPtr d = xmlReadMemory("<r/>", 4, "t", NULL, 0);
        xmlXPathContextPtr c = d ? xmlXPathNewContext(d) : NULL;
        /* NOTE: xmlXPathEvalExpression(NULL, non-NULL ctx) is upstream UB —
         * xmlXPathNewParserContext stores the NULL expression and parsing
         * dereferences it, so the oracle itself segfaults. The defined
         * NULL-context case is covered by A10; only the defined eval path
         * is exercised here. */
        if (c) {
            xmlXPathObjectPtr o = xmlXPathEvalExpression(BAD_CAST "//missing", c);
            printf("F4 eval missing=%s\n", o ? "(ptr)" : "(nil)");
            if (o) {
                printf("F4 eval type=%d nodes=%d\n", o->type,
                       o->type == XPATH_NODESET ? o->nodesetval ? o->nodesetval->nodeNr : 0 : -1);
                xmlXPathFreeObject(o);
            }
            xmlXPathFreeContext(c);
        }
        if (d) xmlFreeDoc(d);
    }

    /* ── F5. save failures ─────────────────────────────────────────────── */
    {
        xmlDocPtr d = xmlReadMemory("<r>t</r>", 8, "t", NULL, 0);
        /* an unwritable path: /nonexistent-dir-xyz/out.xml */
        printf("F5 saveToFilename=%s\n",
               (void *)xmlSaveToFilename("/nonexistent-dir-xyz/out.xml", "UTF-8", 0)
                   ? "(ptr)" : "(nil)");
        int rc = xmlSaveFormatFileEnc("/nonexistent-dir-xyz/out.xml", d, "UTF-8", 1);
        printf("F5 saveFormatFile=%d\n", rc);
        if (d) xmlFreeDoc(d);
    }

    /* ── F6. xmlNodeDump on a detached node with extreme flags ─────────── */
    {
        xmlNodePtr n = xmlNewChild(NULL, NULL, BAD_CAST "r", BAD_CAST "txt");
        xmlBufferPtr b = xmlBufferCreate();
        if (n && b) {
            int d1 = xmlNodeDump(b, NULL, n, 0, 0);
            printf("F6 dump0=%d content=[%s]\n", d1,
                   b->content ? (const char *)b->content : "(null)");
            xmlBufferEmpty(b);
            int d2 = xmlNodeDump(b, NULL, n, 1000000, 0);
            printf("F6 dump-huge-depth=%d\n", d2);
            xmlBufferEmpty(b);
            int d3 = xmlNodeDump(b, NULL, n, 0, 0x7FFFFFFF);
            printf("F6 dump-huge-format=%d\n", d3);
        }
        if (b) xmlBufferFree(b);
        if (n) xmlFreeNode(n);
    }

    /* ── F7. DTD parse failures ────────────────────────────────────────── */
    printf("F7 parseDTD NULL NULL=%s\n",
           (void *)xmlParseDTD(NULL, NULL) ? "(ptr)" : "(nil)");
    printf("F7 parseDTD missing file=%s\n",
           (void *)xmlParseDTD(NULL, BAD_CAST "/nonexistent-dir-xyz/missing.dtd")
               ? "(ptr)" : "(nil)");
    printf("F7 createIntSubset existing-name=%s\n",
           (void *)xmlCreateIntSubset(NULL, BAD_CAST "r", NULL, NULL) ? "(ptr)" : "(nil)");

    /* ── F8. regexp compile failures ───────────────────────────────────── */
    {
        xmlRegexpPtr r1 = xmlRegexpCompile(BAD_CAST "[");
        printf("F8 regexp '['=%s\n", r1 ? "(ptr)" : "(nil)");
        if (r1) xmlRegFreeRegexp(r1);
        xmlRegexpPtr r2 = xmlRegexpCompile(BAD_CAST "(a");
        printf("F8 regexp '(a'=%s\n", r2 ? "(ptr)" : "(nil)");
        if (r2) xmlRegFreeRegexp(r2);
        xmlRegexpPtr r3 = xmlRegexpCompile(BAD_CAST "a{2,1}");
        printf("F8 regexp 'a{2,1}'=%s\n", r3 ? "(ptr)" : "(nil)");
        if (r3) xmlRegFreeRegexp(r3);
    }

    /* ── F9. reader failure paths ──────────────────────────────────────── */
    {
        xmlTextReaderPtr r = xmlReaderForMemory("<<<", 3, "t", NULL, 0);
        printf("F9 reader=%s\n", r ? "(ptr)" : "(nil)");
        if (r) {
            int ret = xmlTextReaderRead(r);
            printf("F9 read=%d\n", ret);
            int again = xmlTextReaderRead(r);
            printf("F9 read-again=%d\n", again);
            xmlFreeTextReader(r);
        }
    }

    /* ── F10. amplification guard ──────────────────────────────────────── */
    {
        const char *doc =
            "<!DOCTYPE r ["
            "<!ENTITY a \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\">"
            "<!ENTITY b \"&a;&a;&a;&a;&a;&a;&a;&a;&a;&a;\">"
            "<!ENTITY c \"&b;&b;&b;&b;&b;&b;&b;&b;&b;&b;\">"
            "<!ENTITY d \"&c;&c;&c;&c;&c;&c;&c;&c;&c;&c;\">"
            "]><r>&d;</r>";
        xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "t", NULL, XML_PARSE_NOENT);
        printf("F10 ampl: doc=%s\n", d ? "(ptr)" : "(nil)");
        if (d) xmlFreeDoc(d);
    }

    /* ── final marker ──────────────────────────────────────────────────── */
    printf("HOSTILE-FAILURE VERDICT PASS\n");
    return 0;
}
