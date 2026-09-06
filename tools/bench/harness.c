/* harness.c — oracle-vs-candidate C ABI benchmark harness (v2).
 *
 * A single source compiled against either upstream libxml2 (oracle) or the
 * libxml-rs drop-in (candidate). It exercises the exact same public C ABI a
 * downstream consumer uses, so the comparison is apples-to-apples: identical
 * harness, identical inputs, identical allocator-visible call path.
 *
 * Operations: parse, xpath, serialize, html, validate, xslt.
 *
 * v2 changes (Phase 16.2):
 *   - warmup phase (discarded) before timed trials,
 *   - N independent timed trials in a single process (single-provider, so no
 *     cross-provider contamination),
 *   - per-trial wall time (CLOCK_MONOTONIC) and CPU time (getrusage diff),
 *   - peak RSS (ru_maxrss) reported once at the end.
 *
 * Output is CSV on stdout (machine-readable for the analysis layer):
 *
 *   op,bytes,iters,trial,wall_ns_per_iter,cpu_ns_per_iter
 *   ... one row per trial ...
 *   RSS,<peak_rss_kib>
 *
 * Usage:
 *   harness <op> <bytes> <iters> <trials> [warmup_iters]
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <sys/resource.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>
#include <libxml/HTMLparser.h>
#include <libxml/valid.h>
#include <libxslt/xslt.h>
#include <libxslt/transform.h>

static double now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (double)ts.tv_sec * 1e9 + (double)ts.tv_nsec;
}

static double cpu_ns(void) {
    struct rusage ru;
    getrusage(RUSAGE_SELF, &ru);
    return (double)(ru.ru_utime.tv_sec + ru.ru_stime.tv_sec) * 1e9 +
           (double)(ru.ru_utime.tv_usec + ru.ru_stime.tv_usec) * 1e3;
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

static void run_op(const char *op, const char *buf, size_t len) {
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

int main(int argc, char **argv) {
    if (argc < 5) {
        fprintf(stderr, "usage: %s <op> <bytes> <iters> <trials> [warmup_iters]\n", argv[0]);
        return 2;
    }
    const char *op = argv[1];
    size_t target = (size_t)strtoull(argv[2], NULL, 10);
    int iters = atoi(argv[3]);
    int trials = atoi(argv[4]);
    int warmup = (argc > 5) ? atoi(argv[5]) : 0;
    if (iters <= 0) iters = 1;
    if (trials <= 0) trials = 1;

    char *buf = NULL;
    size_t len = 0;
    if (strcmp(op, "html") == 0) {
        buf = make_html(target, &len);
    } else {
        buf = make_xml(target, &len);
    }
    if (!buf) return 1;

    /* Warmup (discarded). */
    for (int i = 0; i < warmup; i++) {
        run_op(op, buf, len);
    }

    /* Independent timed trials. */
    for (int t = 0; t < trials; t++) {
        double w0 = now_ns();
        double c0 = cpu_ns();
        for (int i = 0; i < iters; i++) {
            run_op(op, buf, len);
        }
        double w1 = now_ns();
        double c1 = cpu_ns();
        double wall_per_iter = (w1 - w0) / (double)iters;
        double cpu_per_iter = (c1 - c0) / (double)iters;
        printf("%s,%zu,%d,%d,%.1f,%.1f\n", op, len, iters, t, wall_per_iter, cpu_per_iter);
    }

    struct rusage ru;
    getrusage(RUSAGE_SELF, &ru);
    printf("RSS,%ld\n", ru.ru_maxrss);

    free(buf);
    xmlCleanupParser();
    return 0;
}
