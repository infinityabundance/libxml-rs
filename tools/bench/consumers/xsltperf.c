/* xsltperf.c — §16.12.2 libxslt compile / precompiled-apply microdriver.
 *
 * The xsltproc CLI cannot separate stylesheet compilation from application, so
 * the libxslt consumer's `stylesheet_compile` and `precompiled_apply` cells are
 * measured through libxslt's public API directly:
 *   compile: xsltParseStylesheetFile() only.
 *   apply:   parse the stylesheet once, parse the document once, then time
 *            xsltApplyStylesheet() N times (the stylesheet stays compiled).
 *
 * Build (inside the perf container, per provider):
 *   cc -O2 xsltperf.c $(pkg-config --cflags --libs libxslt libxml-2.0) -o xsltperf
 *
 * Usage:
 *   xsltperf --mode compile --xsl S.xsl --reps N
 *   xsltperf --mode apply   --xsl S.xsl --doc D.xml --reps N [--out FILE]
 * Prints: {"mode":"...","ok":1,"ms":<best>}
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/transform.h>
#include <libxslt/xsltutils.h>

static double now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return ts.tv_sec * 1000.0 + ts.tv_nsec / 1e6;
}

static const char *arg(int argc, char **argv, const char *k) {
    for (int i = 1; i + 1 < argc; i++)
        if (strcmp(argv[i], k) == 0) return argv[i + 1];
    return NULL;
}

int main(int argc, char **argv) {
    const char *mode = arg(argc, argv, "--mode");
    const char *xslp = arg(argc, argv, "--xsl");
    const char *docp = arg(argc, argv, "--doc");
    const char *outp = arg(argc, argv, "--out");
    int reps = atoi(arg(argc, argv, "--reps") ? arg(argc, argv, "--reps") : "1");
    if (!mode || !xslp || reps < 1) { fprintf(stderr, "usage error\n"); return 2; }

    xmlInitParser();
    xsltInit();
    double best = -1.0;

    if (strcmp(mode, "compile") == 0) {
        for (int i = 0; i < reps; i++) {
            double t0 = now_ms();
            xsltStylesheetPtr st = xsltParseStylesheetFile((const xmlChar *)xslp);
            double dt = now_ms() - t0;
            if (!st) { fprintf(stderr, "compile failed\n"); return 3; }
            xsltFreeStylesheet(st);
            if (best < 0 || dt < best) best = dt;
        }
        printf("{\"mode\":\"compile\",\"ok\":1,\"ms\":%.6f}\n", best);
        return 0;
    }

    if (strcmp(mode, "apply") == 0) {
        if (!docp) { fprintf(stderr, "--doc required for apply\n"); return 2; }
        xsltStylesheetPtr st = xsltParseStylesheetFile((const xmlChar *)xslp);
        if (!st) { fprintf(stderr, "compile failed\n"); return 3; }
        xmlDocPtr doc = xmlReadFile(docp, NULL, XML_PARSE_NOENT | XML_PARSE_NONET);
        if (!doc) { fprintf(stderr, "doc parse failed\n"); return 4; }
        for (int i = 0; i < reps; i++) {
            double t0 = now_ms();
            xmlDocPtr res = xsltApplyStylesheet(st, doc, NULL);
            double dt = now_ms() - t0;
            if (!res) { fprintf(stderr, "apply failed\n"); return 5; }
            if (i == 0 && outp) xsltSaveResultToFilename(outp, res, st, 0);
            xmlFreeDoc(res);
            if (best < 0 || dt < best) best = dt;
        }
        xmlFreeDoc(doc);
        xsltFreeStylesheet(st);
        printf("{\"mode\":\"apply\",\"ok\":1,\"ms\":%.6f}\n", best);
        return 0;
    }

    fprintf(stderr, "unknown mode %s\n", mode);
    return 2;
}
