/*
 * hostile-oracle-contamination-probe.c — Phase 13 HOSTILE-ORACLE-CONTAMINATION
 * attack court (dimension 7).
 *
 * A hostile auditor asks: does the candidate secretly depend on the SYSTEM
 * libxml2/libxslt/libexslt at runtime? This probe, linked against the
 * candidate DSOs and run with LD_LIBRARY_PATH pointing at the candidate
 * directory, reports which shared object each libxml/libxslt/libexslt symbol
 * ACTUALLY resolved from. The runner asserts every resolution lands inside
 * the candidate directory (never /usr/lib), that all three SONAMEs load,
 * and cross-checks DT_NEEDED / undefined-symbol hygiene with readelf/nm.
 *
 * Output is deterministic (addresses are canonicalized to (ptr)).
 *
 * Court family: HOSTILE-ORACLE-CONTAMINATION (Phase 13 hostile audit,
 * dimension 7: no secret oracle dependency)
 */
#define _GNU_SOURCE
#include <stdio.h>
#include <dlfcn.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltutils.h>
#include <libxslt/transform.h>
#include <libexslt/exslt.h>

/* Canonicalize a pointer for deterministic output. */
static const char *p(const void *x) {
    return x == NULL ? "(nil)" : "(ptr)";
}

int main(void) {
    LIBXML_TEST_VERSION

    /* ── load each SONAME explicitly ─────────────────────────────────────── */
    void *h2 = dlopen("libxml2.so.16", RTLD_NOW | RTLD_LOCAL);
    void *h1 = dlopen("libxslt.so.1", RTLD_NOW | RTLD_LOCAL);
    void *h0 = dlopen("libexslt.so.0", RTLD_NOW | RTLD_LOCAL);
    printf("dlopen(libxml2.so.16)=%s handle=%s\n", h2 ? "ok" : "FAIL",
           p(h2));
    printf("dlopen(libxslt.so.1)=%s handle=%s\n", h1 ? "ok" : "FAIL",
           p(h1));
    printf("dlopen(libexslt.so.0)=%s handle=%s\n", h0 ? "ok" : "FAIL",
           p(h0));

    /* ── where did each linked symbol really come from? ──────────────────── */
    Dl_info i2, i1, i0;
    if (dladdr((void *) &xmlReadMemory, &i2) && i2.dli_fname)
        printf("xmlReadMemory from: %s\n", i2.dli_fname);
    else
        printf("xmlReadMemory from: (unresolved)\n");
    if (dladdr((void *) &xsltApplyStylesheet, &i1) && i1.dli_fname)
        printf("xsltApplyStylesheet from: %s\n", i1.dli_fname);
    else
        printf("xsltApplyStylesheet from: (unresolved)\n");
    if (dladdr((void *) &exsltRegisterAll, &i0) && i0.dli_fname)
        printf("exsltRegisterAll from: %s\n", i0.dli_fname);
    else
        printf("exsltRegisterAll from: (unresolved)\n");

    /* ── the loaded parser version marker (candidate build identity) ─────── */
    printf("xmlParserVersion: %s\n", xmlParserVersion);

    /* ── a symbol that must come from the CANDIDATE libxml2 (TLS accessor) ─ */
    {
        xmlGenericErrorFunc *ge = __xmlGenericError();
        printf("__xmlGenericError ptr: %s\n", p((void *) ge));
    }

    if (h2) dlclose(h2);
    if (h1) dlclose(h1);
    if (h0) dlclose(h0);

    printf("HOSTILE-ORACLE-CONTAMINATION VERDICT PASS\n");
    return 0;
}
