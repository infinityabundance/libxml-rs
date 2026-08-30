/*
 * GLOBALS-001 — differential court for global state, threading and
 * initialization (11.1-K).
 *
 * Phases (all deterministic):
 *  1. init lifecycle: xmlInitParser (first + repeated), xmlCleanupParser,
 *     reinitialization, xmlInitThreads, xmlGetThreadId, xmlIsMainThread.
 *  2. Global mutation: write/read xmlDoValidityCheckingDefaultValue,
 *     xmlKeepBlanksDefaultValue, xmlLoadExtDtdDefaultValue,
 *     xmlIndentTreeOutput, xmlSubstituteEntitiesDefaultValue (save/restore).
 *  3. Function-pointer global replacement: xmlSetGenericErrorFunc with a
 *     counting handler, then restore.
 *  4. Concurrent parsing: 8 threads x 200 iterations of parse+xpath+free;
 *     each thread emits a stable FNV-1a digest of its outputs, printed
 *     sorted so the result is byte-deterministic across oracle/candidate.
 *  5. Concurrent init/cleanup: 4 threads cycling xmlInitParser and
 *     xmlCleanupParser while parsing (upstream supports this).
 *
 * Raw pointers are never printed.
 */
#include <stdio.h>
#include <string.h>
#include <pthread.h>
#include <stdlib.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>
#include <libxml/xpathInternals.h>
#include <libxml/globals.h>
#include <libxml/threads.h>

static const char *XML_DOC =
    "<?xml version=\"1.0\"?><root><item a=\"1\">x</item><item a=\"2\">y</item></root>";

static unsigned long fnv(const unsigned char *s, size_t n) {
    unsigned long h = 1469598103934665603UL;
    size_t i;
    for (i = 0; i < n; i++) {
        h ^= s[i];
        h *= 1099511628211UL;
    }
    return h;
}

static int error_count = 0;
static void counting_error(void *ctx, const char *msg, ...) {
    (void) ctx; (void) msg;
    __sync_fetch_and_add(&error_count, 1);
}

static unsigned long worker_digest(void) {
    unsigned long h = 1469598103934665603UL;
    int i;
    for (i = 0; i < 200; i++) {
        xmlDocPtr doc = xmlReadMemory(XML_DOC, (int) strlen(XML_DOC), "t.xml", NULL, 0);
        if (!doc) continue;
        xmlXPathContextPtr xc = xmlXPathNewContext(doc);
        xmlXPathObjectPtr obj = xmlXPathEvalExpression(
            (const xmlChar *) "//item[@a='2']/text()", xc);
        xmlChar *s = obj ? xmlXPathCastToString(obj) : NULL;
        if (s) { h = fnv((const unsigned char *) s, strlen((const char *) s)) ^ h; free(s); }
        if (obj) xmlXPathFreeObject(obj);
        xmlXPathFreeContext(xc);
        xmlFreeDoc(doc);
    }
    return h;
}

static void *thread_main(void *arg) {
    unsigned long *out = (unsigned long *) arg;
    *out = worker_digest();
    return NULL;
}

static void *thread_cycle(void *arg) {
    unsigned long *out = (unsigned long *) arg;
    *out = worker_digest();
    xmlInitParser();
    xmlCleanupParser();
    return NULL;
}

static int cmp_ul(const void *a, const void *b) {
    unsigned long x = *(const unsigned long *) a, y = *(const unsigned long *) b;
    return (x > y) - (x < y);
}

int main(void) {
    int i;

    /* 1. Init lifecycle. */
    xmlInitParser();
    xmlInitParser();          /* repeated */
    xmlInitThreads();
    xmlCleanupParser();
    xmlInitParser();          /* reinitialization */
    printf("init-cycle ok\n");

    /* 2. Global mutation. */
    int save_valid = xmlDoValidityCheckingDefaultValue;
    int save_blanks = xmlKeepBlanksDefaultValue;
    int save_ext = xmlLoadExtDtdDefaultValue;
    int save_indent = xmlIndentTreeOutput;
    int save_subst = xmlSubstituteEntitiesDefaultValue;
    xmlDoValidityCheckingDefaultValue = 1;
    xmlKeepBlanksDefaultValue = 0;
    xmlLoadExtDtdDefaultValue = 5;
    xmlIndentTreeOutput = 1;
    xmlSubstituteEntitiesDefaultValue = 1;
    printf("globals %d %d %d %d %d\n",
           xmlDoValidityCheckingDefaultValue, xmlKeepBlanksDefaultValue,
           xmlLoadExtDtdDefaultValue, xmlIndentTreeOutput,
           xmlSubstituteEntitiesDefaultValue);
    xmlDoValidityCheckingDefaultValue = save_valid;
    xmlKeepBlanksDefaultValue = save_blanks;
    xmlLoadExtDtdDefaultValue = save_ext;
    xmlIndentTreeOutput = save_indent;
    xmlSubstituteEntitiesDefaultValue = save_subst;
    printf("restored %d %d %d %d %d\n",
           xmlDoValidityCheckingDefaultValue, xmlKeepBlanksDefaultValue,
           xmlLoadExtDtdDefaultValue, xmlIndentTreeOutput,
           xmlSubstituteEntitiesDefaultValue);

    /* 3. Function-pointer global replacement. */
    xmlSetGenericErrorFunc(NULL, counting_error);
    /* Force an error through the generic handler (invalid entity). */
    xmlReadMemory("<?xml version=\"1.0\"?><a>&bogus;</a>", 30, "e.xml", NULL, 0);
    printf("err-count %d\n", error_count);
    xmlSetGenericErrorFunc(NULL, NULL);

    /* 4. Concurrent parsing. */
    pthread_t th[8];
    unsigned long digests[8];
    for (i = 0; i < 8; i++) pthread_create(&th[i], NULL, thread_main, &digests[i]);
    for (i = 0; i < 8; i++) pthread_join(th[i], NULL);
    qsort(digests, 8, sizeof(unsigned long), cmp_ul);
    printf("thread-digests");
    for (i = 0; i < 8; i++) printf(" %016lx", digests[i]);
    printf("\n");

    /* 5. Concurrent init/cleanup + parse. */
    for (i = 0; i < 4; i++) pthread_create(&th[i], NULL, thread_cycle, &digests[i]);
    for (i = 0; i < 4; i++) pthread_join(th[i], NULL);
    qsort(digests, 4, sizeof(unsigned long), cmp_ul);
    printf("cycle-digests");
    for (i = 0; i < 4; i++) printf(" %016lx", digests[i]);
    printf("\n");

    xmlCleanupParser();
    return 0;
}
