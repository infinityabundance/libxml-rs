/*
 * ALLOCATOR-DEFAULT-001 — differential probe of the DEFAULT allocator
 * contract (11.1-Z.3, R-000178).
 *
 * Compiled twice: against the oracle DSO (system libxml2, whose default
 * `xmlMalloc`/`xmlFree`/`xmlRealloc`/`xmlMemStrdup` variables are the plain
 * C runtime functions — globals.c) and against the candidate DSO
 * (liblibxml_rs, whose `*Default` bodies are libc wrappers). The output is
 * deterministic: it prints only RELATIVE facts (NULL/non-NULL patterns,
 * content preservation, routing counters, and the exact values of
 * xmlMemSize/xmlMemUsed/xmlMemBlocks, which the oracle keeps at 0 while the
 * default allocator is installed) — never pointer values or addresses.
 *
 * The two program outputs must be byte-identical.
 *
 * Exercises:
 *   (1) many allocation sizes + zero-size allocation (C malloc contract);
 *   (2) grow realloc / shrink realloc / realloc-to-zero (C realloc
 *       contract: realloc(p,0) frees and returns NULL on glibc);
 *   (3) realloc(NULL, n) == malloc; realloc failure leaves the old block
 *       intact; malloc(SIZE_MAX) == NULL;
 *   (4) strdup content + strdup(NULL) == NULL; free(NULL) no-op;
 *   (5) long allocation/free churn (100k blocks);
 *   (6) direct exported-variable assignment (xmlMalloc = custom) routes
 *       actual allocations, and the debug counters stay 0 (upstream: custom
 *       hooks never touch debugMemSize);
 *   (7) xmlMemSize/xmlMemUsed/xmlMemBlocks exactness under the default
 *       allocator (all 0, matching the oracle's plain-malloc default);
 *   (8) the display entry points (xmlMemDisplay/xmlMemDisplayLast/xmlMemShow)
 *       write nothing (upstream 2.15.0 removed the feature).
 */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/xmlmemory.h>

static int nmalloc = 0, nfree = 0;
static void *counting_malloc(size_t s) { nmalloc++; return malloc(s); }
static void counting_free(void *p) { nfree++; free(p); }

/* Suppress -Walloc-size-larger-than for the intentional SIZE_MAX probes. */
static size_t huge_size(void) { return (size_t)-1; }

int main(void) {
    void *p, *q;
    unsigned char buf[64];
    unsigned long i;
    FILE *f;
    long pos;

    /* (1) counters start at 0 with the default allocator (debugMemSize). */
    printf("stats0 used=%d blocks=%d\n", xmlMemUsed(), xmlMemBlocks());

    /* (2) many allocation sizes + zero-size. */
    for (i = 1; i <= 16384; i <<= 1) {
        p = xmlMalloc((size_t)i);
        printf("m%lu nonnull=%d\n", i, p != NULL);
        if (p) xmlFree(p);
    }
    p = xmlMalloc(0);
    printf("m0 nonnull=%d\n", p != NULL);
    if (p) xmlFree(p);

    /* (3) grow realloc preserves content. */
    memset(buf, 0x5a, sizeof(buf));
    p = xmlMalloc(64);
    if (p) memcpy(p, buf, 64);
    q = xmlRealloc(p, 256);
    printf("realloc-grow nonnull=%d content=%d\n",
           q != NULL, q != NULL && memcmp(q, buf, 64) == 0);

    /* (4) shrink realloc preserves the leading content. */
    p = xmlRealloc(q, 16);
    printf("realloc-shrink nonnull=%d content=%d\n",
           p != NULL, p != NULL && memcmp(p, buf, 16) == 0);

    /* (5) realloc(p, 0): glibc frees the block and returns NULL. */
    q = xmlRealloc(p, 0);
    printf("realloc-zero null=%d\n", q == NULL);

    /* (6) realloc(NULL, n) == malloc. */
    p = xmlRealloc(NULL, 32);
    printf("realloc-null nonnull=%d\n", p != NULL);
    xmlFree(p);

    /* (7) realloc failure: old block stays allocated and intact. */
    memset(buf, 0x3c, sizeof(buf));
    p = xmlMalloc(16);
    if (p) memcpy(p, buf, 16);
    q = xmlRealloc(p, huge_size());
    printf("realloc-fail null=%d old-intact=%d content=%d\n",
           q == NULL, q == NULL, p != NULL && memcmp(p, buf, 16) == 0);
    xmlFree(p);

    /* (8) malloc failure. */
    p = xmlMalloc(huge_size());
    printf("malloc-huge null=%d\n", p == NULL);

    /* (9) strdup content + NULL handling (upstream xmlPosixStrdup). */
    p = xmlMemStrdup("hello allocator");
    printf("strdup ok=%d\n", p != NULL && strcmp((char *)p, "hello allocator") == 0);
    xmlFree(p);
    p = xmlMemStrdup(NULL);
    printf("strdup-null null=%d\n", p == NULL);

    /* (10) free(NULL) is a no-op. */
    xmlFree(NULL);
    printf("free-null ok=1\n");

    /* (11) long allocation/free churn (100k blocks, mixed sizes). */
    for (i = 0; i < 100000; i++) {
        p = xmlMalloc(1 + (i % 128));
        if (!p) break;
        *(volatile unsigned char *)p = (unsigned char)i;
        xmlFree(p);
    }
    printf("churn done=%d\n", i == 100000);

    /* (12) direct exported-variable assignment routes actual allocations. */
    nmalloc = nfree = 0;
    xmlMalloc = counting_malloc;
    xmlFree = counting_free;
    p = xmlMalloc(48);
    printf("direct routed=%d nonnull=%d\n", nmalloc == 1, p != NULL);
    xmlFree(p);
    printf("direct free=%d\n", nfree == 1);

    /* (13) exactness: with the default allocator the debug counters stay 0
     * even after hundreds of allocations, and xmlMemSize is 0 for
     * default-allocator blocks (upstream: plain malloc carries no MEMHDR). */
    printf("stats-final used=%d blocks=%d\n", xmlMemUsed(), xmlMemBlocks());
    p = xmlMalloc(80);
    printf("size-live=%zu size-null=%zu\n", xmlMemSize(p), xmlMemSize(NULL));
    xmlFree(p);

    /* (14) display entry points write nothing (upstream 2.15.0 no-ops). */
    f = tmpfile();
    xmlMemDisplay(f);
    xmlMemDisplayLast(f, -1);
    xmlMemShow(f, 10);
    pos = f ? ftell(f) : -1;
    printf("display-bytes=%ld\n", pos);
    if (f) fclose(f);

    return 0;
}
