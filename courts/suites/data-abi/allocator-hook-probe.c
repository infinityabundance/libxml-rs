/*
 * ALLOCATOR-HOOK-001 — differential probe of the allocator-hook contract
 * (11.1-Z.2, R-000176).
 *
 * Compiled twice: against the oracle DSO (system libxml2) and against the
 * candidate DSO (liblibxml_rs). The output is deterministic: it prints only
 * RELATIVE facts (return codes, NULL/non-NULL patterns, whether a hook was
 * observed, whether the exported variables equal the installed callbacks) —
 * never function-pointer VALUES or addresses.
 *
 * Exercises the single-source-of-truth model:
 *   (a) xmlMemSetup → exported variables → actual allocations route through
 *       the installed hooks;
 *   (b) the GC variant with a distinct mallocAtomicFunc;
 *   (c) DIRECT public-variable assignment (xmlMalloc = custom) → xmlMemGet
 *       and actual allocations observe it;
 *   (d) NULL-hook rejection returns -1 (upstream xmlmemory.c).
 */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/xmlmemory.h>

static int nmalloc = 0, nfree = 0, nrealloc = 0, nstrdup = 0;

static void *my_malloc(size_t s) { nmalloc++; return malloc(s); }
static void *my_malloc_atomic(size_t s) { nmalloc++; return malloc(s); }
static void my_free(void *p) { nfree++; free(p); }
static void *my_realloc(void *p, size_t s) { nrealloc++; return realloc(p, s); }
static char *my_strdup(const char *s) { nstrdup++; return strdup(s); }

int main(void) {
    xmlFreeFunc f;
    xmlMallocFunc m, ma;
    xmlReallocFunc r;
    xmlStrdupFunc d;
    int rc;
    void *p;

    /* (0) initial xmlMemGet sanity: four slots non-NULL */
    rc = xmlMemGet(&f, &m, &r, &d);
    printf("get0 rc=%d nonnull=%d%d%d%d\n", rc, f != NULL, m != NULL, r != NULL, d != NULL);

    /* (1) xmlMemSetup installs the exported variables and routes allocations */
    rc = xmlMemSetup(my_free, my_malloc, my_realloc, my_strdup);
    xmlMemGet(&f, &m, &r, &d);
    printf("setup rc=%d memget==installed %d%d%d%d\n", rc,
           f == my_free, m == my_malloc, r == my_realloc, d == my_strdup);
    printf("vars==installed %d%d%d%d%d\n",
           xmlFree == my_free, xmlMalloc == my_malloc,
           xmlMallocAtomic == my_malloc, xmlRealloc == my_realloc,
           xmlMemStrdup == my_strdup);

    nmalloc = nfree = nrealloc = nstrdup = 0;
    p = xmlMalloc(64);
    printf("malloc routed=%d ptr-nonnull=%d\n", nmalloc == 1, p != NULL);
    p = xmlRealloc(p, 128);
    printf("realloc routed=%d\n", nrealloc == 1);
    xmlFree(p);
    printf("free routed=%d\n", nfree == 1);
    p = xmlMemStrdup("hi");
    printf("strdup routed=%d\n", nstrdup == 1);
    xmlFree(p);

    /* (2) GC variant: dedicated mallocAtomicFunc slot */
    nmalloc = 0;
    rc = xmlGcMemSetup(my_free, my_malloc, my_malloc_atomic, my_realloc, my_strdup);
    xmlGcMemGet(&f, &m, &ma, &r, &d);
    printf("gcsetup rc=%d atomic-var==atomic=%d memget==installed %d%d%d%d%d\n", rc,
           xmlMallocAtomic == my_malloc_atomic,
           f == my_free, m == my_malloc, ma == my_malloc_atomic,
           r == my_realloc, d == my_strdup);
    p = xmlMallocAtomic(16);
    printf("atomic routed=%d\n", nmalloc == 1);
    xmlFree(p);

    /* (3) direct public-variable assignment is the same override mechanism */
    nmalloc = 0;
    xmlMalloc = my_malloc;
    xmlFree = my_free;
    xmlMemGet(&f, &m, NULL, NULL);
    printf("direct memget==assigned %d%d\n", f == my_free, m == my_malloc);
    p = xmlMalloc(32);
    printf("direct routed=%d\n", nmalloc == 1);
    xmlFree(p);

    /* (4) NULL hooks are rejected with -1 (upstream xmlmemory.c) */
    rc = xmlMemSetup(NULL, my_malloc, my_realloc, my_strdup);
    printf("null-setup rc=%d\n", rc);
    rc = xmlGcMemSetup(my_free, my_malloc, NULL, my_realloc, my_strdup);
    printf("null-gc-setup rc=%d\n", rc);

    return 0;
}
