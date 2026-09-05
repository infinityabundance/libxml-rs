/* harness.c — oracle-vs-candidate C ABI benchmark harness.
 *
 * A single source compiled against either upstream libxml2 (oracle) or the
 * libxml-rs drop-in (candidate). It exercises the exact same public C ABI a
 * downstream consumer uses, so the comparison is apples-to-apples: identical
 * harness, identical inputs, identical allocator-visible call path.
 *
 * Operations: parse, xpath, serialize, html, validate, xslt.
 *
 * Output is CSV on stdout (machine-readable for the Pareto matrix):
 *   op,bytes,iters,mean_ns,throughput_bytes_per_sec
 *
 * Usage:
 *   harness <op> <bytes> <iters> [seed]
 *
 * The generated document size is approximate (`bytes` target); the actual
 * byte length is reported in the CSV row.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>
#include <libxml/HTMLparser.h>
#include <libxslt/xslt.h>
#include <libxslt/transform.h>

static double now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

/* Build an element-heavy XML document approximating `bytes` bytes. */
static char *make_xml(size_t target, size_t *out_len) {
    size_t cap = target + 64;
    char *buf = malloc(cap);
    if (!buf) return NULL;
    size_t n = 0;
    n += snprintf(buf + n, cap - n, "<root>");
    int i = 0;
    while (n < target) {
        int w = snprintf(buf + n, cap - n, "<item id=\"i%d\">value%d</item>", i, i);
        if (w <= 0) break;
        n += (size_t)w;
        i++;
    }
    snprintf(buf + n, cap - n, "</root>");
    n = strlen(buf);
    *out_len = n;
    return buf;
}

static char *make_html(size_t target, size_t *out_len) {
    size_t cap = target + 64;
    char *buf = malloc(cap);
    if (!buf) return NULL;
    size_t n = 0;
    n += snprintf(buf + n, cap - n, "<html><body><table>");
    int i = 0;
    while (n < target) {
        int w = snprintf(buf + n, cap - n, "<tr><td>cell%d</td></tr>", i);
        if (w <= 0) break;
        n += (size_t)w;
        i++;
    }
    snprintf(buf + n, cap - n, "</table></body></html>");
    n = strlen(buf);
    *out_len = n;
    return buf;
}

static const char XSLT_DOC[] =
    "<?xml version=\"1.0\"?>"
    "<xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">"
    "  <xsl:template match=\"/\">"
    "    <out><xsl:for-each select=\"root/item\"><v><xsl:value-of select=\".\"/></v></xsl:for-each></out>"
    "  </xsl:template>"
    "</xsl:stylesheet>";

static xmlDocPtr parse_mem(const char *buf, size_t len) {
    return xmlReadMemory(buf, (int)len, NULL, NULL, 0);
}

int main(int argc, char **argv) {
    if (argc < 4) {
        fprintf(stderr, "usage: %s <op> <bytes> <iters>\n", argv[0]);
        return 2;
    }
    const char *op = argv[1];
    size_t target = (size_t)strtoull(argv[2], NULL, 10);
    int iters = atoi(argv[3]);
    if (iters <= 0) iters = 1;

    char *buf = NULL;
    size_t len = 0;
    double t0, t1;
    int i;

    if (strcmp(op, "parse") == 0 || strcmp(op, "xpath") == 0 ||
        strcmp(op, "serialize") == 0 || strcmp(op, "validate") == 0 ||
        strcmp(op, "xslt") == 0) {
        buf = make_xml(target, &len);
    } else if (strcmp(op, "html") == 0) {
        buf = make_html(target, &len);
    } else {
        fprintf(stderr, "unknown op: %s\n", op);
        return 2;
    }
    if (!buf) return 1;

    t0 = now_ns();
    for (i = 0; i < iters; i++) {
        if (strcmp(op, "parse") == 0) {
            xmlDocPtr d = parse_mem(buf, len);
            if (d) xmlFreeDoc(d);
        } else if (strcmp(op, "html") == 0) {
            xmlDocPtr d = htmlReadMemory(buf, (int)len, NULL, NULL, 0);
            if (d) xmlFreeDoc(d);
        } else if (strcmp(op, "serialize") == 0) {
            xmlDocPtr d = parse_mem(buf, len);
            if (d) {
                xmlChar *mem = NULL;
                int sz = 0;
                xmlDocDumpMemory(d, &mem, &sz);
                if (mem) xmlFree(mem);
                xmlFreeDoc(d);
            }
        } else if (strcmp(op, "xpath") == 0) {
            xmlDocPtr d = parse_mem(buf, len);
            if (d) {
                xmlXPathContextPtr ctx = xmlXPathNewContext(d);
                xmlXPathObjectPtr obj = xmlXPathEvalExpression((const xmlChar *)"count(//item)", ctx);
                if (obj) xmlXPathFreeObject(obj);
                xmlXPathFreeContext(ctx);
                xmlFreeDoc(d);
            }
        } else if (strcmp(op, "validate") == 0) {
            xmlDocPtr d = parse_mem(buf, len);
            if (d) {
                xmlValidCtxtPtr v = xmlNewValidCtxt();
                xmlValidateDocument(v, d);
                xmlFreeValidCtxt(v);
                xmlFreeDoc(d);
            }
        } else if (strcmp(op, "xslt") == 0) {
            xmlDocPtr sd = xmlReadMemory(XSLT_DOC, (int)strlen(XSLT_DOC), NULL, NULL, 0);
            xsltStylesheetPtr style = xsltParseStylesheetDoc(sd);
            xmlDocPtr d = parse_mem(buf, len);
            if (d) {
                xmlDocPtr out = xsltApplyStylesheet(style, d, NULL);
                if (out) xmlFreeDoc(out);
                xmlFreeDoc(d);
            }
            if (style) xsltFreeStylesheet(style);
        }
    }
    t1 = now_ns();
    double total = t1 - t0;
    double mean = total / (double)iters;
    double thrpt = (mean > 0.0) ? ((double)len / (mean / 1e9)) : 0.0;

    printf("%s,%zu,%d,%.1f,%.1f\n", op, len, iters, mean, thrpt);
    free(buf);
    xmlCleanupParser();
    return 0;
}
