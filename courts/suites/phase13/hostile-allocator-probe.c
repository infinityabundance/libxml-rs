/*
 * hostile-allocator-probe.c — Phase 13 HOSTILE-ALLOCATOR attack court.
 *
 * Installs deliberately hostile allocators through xmlMemSetup and verifies
 * the library survives and fails identically to the oracle:
 *
 *   H1. xmlMemSetup with NULL function pointers must be rejected (-1) and
 *       must leave the running allocator untouched.
 *   H2. malloc that always fails: every allocation API must return NULL,
 *       the library must not crash, and restoring the default allocator
 *       must fully revive the library.
 *   H3. malloc that fails above a size threshold: small documents parse,
 *       large ones fail — deterministic on both sides because the threshold
 *       is size-based, not call-count-based.
 *   H4. realloc that always fails: growth paths (xmlBufferAdd, xmlStrncat)
 *       must fail with the upstream error contract.
 *   H5. strdup that always fails: xmlStrdup/xmlMemStrdup return NULL.
 *   H6. the hostile allocator itself must be observable through xmlMemGet.
 *
 * The hostile callbacks use libc directly (never the library allocator, to
 * avoid recursion) and failure modes are deliberately SIZE-based, never
 * call-count-based: the oracle and the candidate legitimately allocate
 * different numbers of blocks internally, so only implementation-
 * independent failure behaviour is compared. Output is deterministic and
 * stderr is compared byte-for-byte.
 *
 * Court family: HOSTILE-ALLOCATOR (Phase 13 hostile audit, dimension 3:
 * allocator substitution)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlmemory.h>
#include <libxml/xmlreader.h>

/* ── hostile allocator state ──────────────────────────────────────────── */
static int fail_all = 0;
static size_t fail_above = 0; /* fail when size > fail_above (0 = off) */
static int fail_realloc = 0;
static int fail_strdup = 0;

static void *h_malloc(size_t size) {
    if (fail_all)
        return NULL;
    if (fail_above && size > fail_above)
        return NULL;
    return malloc(size);
}

static void *h_realloc(void *ptr, size_t size) {
    if (fail_realloc)
        return NULL;
    return realloc(ptr, size);
}

static void h_free(void *ptr) {
    free(ptr);
}

static char *h_strdup(const char *str) {
    if (fail_strdup)
        return NULL;
    if (str == NULL)
        return NULL;
    size_t n = strlen(str) + 1;
    char *p = malloc(n);
    if (p == NULL)
        return NULL;
    memcpy(p, str, n);
    return p;
}

/* default allocator capture */
static xmlFreeFunc def_free;
static xmlMallocFunc def_malloc;
static xmlReallocFunc def_realloc;
static xmlStrdupFunc def_strdup;

static void capture_default(void) {
    xmlMemGet(&def_free, &def_malloc, &def_realloc, &def_strdup);
}

static void restore_default(void) {
    xmlMemSetup(def_free, def_malloc, def_realloc, def_strdup);
}

static void install_hostile(void) {
    xmlMemSetup(h_free, h_malloc, h_realloc, h_strdup);
}

int main(void) {
    LIBXML_TEST_VERSION
    capture_default();

    /* ── H1. NULL function pointers are rejected ───────────────────────── */
    {
        int r = xmlMemSetup(NULL, NULL, NULL, NULL);
        printf("H1 setup-NULL=%d\n", r);
        xmlFreeFunc f; xmlMallocFunc m; xmlReallocFunc r2; xmlStrdupFunc s;
        xmlMemGet(&f, &m, &r2, &s);
        printf("H1 allocator-unchanged=%s\n",
               (f == def_free && m == def_malloc && r2 == def_realloc &&
                s == def_strdup)
                   ? "(yes)"
                   : "(no)");
    }

    /* ── H2. always-failing malloc ─────────────────────────────────────── */
    fail_all = 1;
    install_hostile();
    printf("H2 xmlReadMemory=%s\n", (void *)xmlReadMemory("<a/>", 4, "t", NULL, 0)
                                        ? "(ptr)" : "(nil)");
    printf("H2 xmlNewDoc=%s\n", (void *)xmlNewDoc(BAD_CAST "1.0") ? "(ptr)" : "(nil)");
    printf("H2 xmlStrdup=%s\n", (void *)xmlStrdup(BAD_CAST "x") ? "(ptr)" : "(nil)");
    printf("H2 xmlMalloc(16)=%s\n", (void *)xmlMalloc(16) ? "(ptr)" : "(nil)");
    printf("H2 xmlMalloc(0)=%s\n", (void *)xmlMalloc(0) ? "(ptr)" : "(nil)");
    printf("H2 xmlBufferCreate=%s\n", (void *)xmlBufferCreate() ? "(ptr)" : "(nil)");
    printf("H2 xmlNewTextReader=%s\n",
           (void *)xmlReaderForMemory("<a/>", 4, "t", NULL, 0) ? "(ptr)" : "(nil)");
    xmlFree(NULL);
    printf("H2 xmlFree(NULL) inert\n");
    restore_default();
    {
        xmlDocPtr d = xmlReadMemory("<a/>", 4, "t", NULL, 0);
        printf("H2 revived parse=%s\n", d ? "(ptr)" : "(nil)");
        if (d) xmlFreeDoc(d);
    }

    /* ── H3. size-threshold malloc failures ────────────────────────────── */
    fail_all = 0;
    fail_above = 64;
    install_hostile();
    {
        xmlDocPtr small = xmlReadMemory("<r/>", 4, "t", NULL, 0);
        printf("H3 small parse=%s\n", small ? "(ptr)" : "(nil)");
        if (small) xmlFreeDoc(small);
        /* "<r><a>this content pushes the tree past the 64-byte ceiling</a></r>" */
        const char *big = "<r><a>this content pushes the tree past the "
                          "64-byte ceiling</a></r>";
        xmlDocPtr large = xmlReadMemory(big, (int)strlen(big), "t", NULL, 0);
        printf("H3 large parse=%s\n", large ? "(ptr)" : "(nil)");
        if (large) xmlFreeDoc(large);
    }
    restore_default();
    fail_above = 0;

    /* ── H4. always-failing realloc ────────────────────────────────────── */
    fail_realloc = 1;
    install_hostile();
    {
        xmlBufferPtr b = xmlBufferCreate();
        printf("H4 buffer=%s\n", b ? "(ptr)" : "(nil)");
        if (b) {
            int r = xmlBufferAdd(b, BAD_CAST "0123456789", 10);
            printf("H4 bufferAdd-grow=%d\n", r);
            xmlBufferFree(b);
        }
        /* xmlStrncat takes ownership of cur and frees it on realloc
         * failure (upstream xmlstring.c) — pass a heap buffer, never a
         * literal, and never touch it afterwards. */
        xmlChar *s = xmlStrdup(BAD_CAST "abc");
        if (s) {
            xmlChar *r = xmlStrncat(s, BAD_CAST "def", 3);
            printf("H4 xmlStrncat=%s\n", r ? "(ptr)" : "(nil)");
        } else {
            printf("H4 xmlStrncat=strdup-failed\n");
        }
    }
    restore_default();
    fail_realloc = 0;

    /* ── H5. always-failing strdup ─────────────────────────────────────── */
    fail_strdup = 1;
    install_hostile();
    printf("H5 xmlStrdup=%s\n", (void *)xmlStrdup(BAD_CAST "x") ? "(ptr)" : "(nil)");
    printf("H5 xmlMemStrdup=%s\n", (void *)xmlMemStrdup(BAD_CAST "x") ? "(ptr)" : "(nil)");
    restore_default();
    fail_strdup = 0;
    printf("H5 revived xmlStrdup=%s\n",
           (void *)xmlStrdup(BAD_CAST "x") ? "(ptr)" : "(nil)");

    /* ── H6. hostile allocator observable through xmlMemGet ────────────── */
    fail_all = 0;
    install_hostile();
    {
        xmlFreeFunc f; xmlMallocFunc m; xmlReallocFunc r2; xmlStrdupFunc s;
        xmlMemGet(&f, &m, &r2, &s);
        printf("H6 get-match=%s\n",
               (f == h_free && m == h_malloc && r2 == h_realloc && s == h_strdup)
                   ? "(yes)"
                   : "(no)");
    }
    restore_default();
    {
        xmlFreeFunc f; xmlMallocFunc m; xmlReallocFunc r2; xmlStrdupFunc s;
        xmlMemGet(&f, &m, &r2, &s);
        printf("H6 restored=%s\n",
               (f == def_free && m == def_malloc && r2 == def_realloc &&
                s == def_strdup)
                   ? "(yes)"
                   : "(no)");
    }

    /* ── final marker ──────────────────────────────────────────────────── */
    printf("HOSTILE-ALLOCATOR VERDICT PASS\n");
    return 0;
}
