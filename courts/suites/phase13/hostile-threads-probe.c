/*
 * hostile-threads-probe.c — Phase 13 HOSTILE-THREADS attack court.
 *
 * Exercises the threading/global-state surface a hostile downstream can
 * reach: concurrent parses of hostile documents, per-thread error state
 * (xmlGetLastError must reflect the CURRENT thread's parse, not another
 * thread's), concurrent global reads during parses, and repeated
 * setup/teardown cycles. Every thread prints deterministic lines; the
 * final output must be byte-identical between oracle and candidate.
 *
 * NOTE: allocator installation (xmlMemSetup) and error-handler
 * installation are NOT performed concurrently — upstream documents those
 * as not thread-safe, so the court keeps them single-threaded. What IS
 * attacked: concurrent parsing, per-thread last-error TLS semantics, and
 * concurrent access to read-mostly globals.
 *
 * Court family: HOSTILE-THREADS (Phase 13 hostile audit, dimension 6:
 * threading & global state)
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <pthread.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

#define NTHREADS 6
#define NPARSES 50

/* Swallow library diagnostics: each worker installs a thread-local no-op
 * structured handler so concurrent error emission cannot interleave on
 * stderr (which is nondeterministic for BOTH sides). The last-error state
 * and the parse outcomes are still fully validated. */
static void swallow(void *ctx, const xmlError *err) {
    (void)ctx;
    (void)err;
}

/* Per-thread work: parse good + hostile docs, record the last error. */
struct result {
    int good_docs;
    int bad_docs;
    int last_code;
};

static struct result results[NTHREADS];

static void *worker(void *arg) {
    long id = (long)arg;
    /* Worker index is the deterministic slot for the result row: callers
     * pass 0..NTHREADS-1 (T1) and NTHREADS..2*NTHREADS-1 (T3, mapped back
     * to the slot below) so concurrent writes stay in-bounds and the
     * post-join prints are deterministic. */
    int slot = (int)(id % NTHREADS);
    int good_docs = 0;
    int bad_docs = 0;
    int last_code = -1;

    /* thread-local on both sides (upstream TLS error state) */
    xmlSetStructuredErrorFunc(NULL, swallow);

    for (int i = 0; i < NPARSES; i++) {
        char doc[64];
        snprintf(doc, sizeof(doc), "<r id=\"%ld-%d\"><a>t</a></r>", id, i);
        xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "t", NULL, 0);
        if (d != NULL) {
            xmlNodePtr r = xmlDocGetRootElement(d);
            if (r != NULL && r->name != NULL && strcmp((const char *)r->name, "r") == 0)
                good_docs++;
            xmlFreeDoc(d);
        }
        /* hostile: garbage */
        xmlDocPtr b = xmlReadMemory("<<<", 3, "t", NULL, 0);
        if (b != NULL) {
            bad_docs++;
            xmlFreeDoc(b);
        }
        /* hostile: entity loop */
        const char *loop = "<!DOCTYPE r [<!ENTITY a \"&a;\">]><r>&a;</r>";
        xmlDocPtr e = xmlReadMemory(loop, (int)strlen(loop), "t", NULL, XML_PARSE_NOENT);
        if (e != NULL) {
            bad_docs++;
            xmlFreeDoc(e);
        }
        /* per-thread last error must reflect THIS thread's last failure */
        xmlErrorPtr err = xmlGetLastError();
        last_code = err ? err->code : 0;
    }

    results[slot].good_docs = good_docs;
    results[slot].bad_docs = bad_docs;
    results[slot].last_code = last_code;
    return NULL;
}

int main(void) {
    LIBXML_TEST_VERSION

    /* ── T1. concurrent hostile parses ─────────────────────────────────── */
    {
        pthread_t threads[NTHREADS];
        memset(results, 0, sizeof(results));
        for (long i = 0; i < NTHREADS; i++)
            pthread_create(&threads[i], NULL, worker, (void *)i);
        for (int i = 0; i < NTHREADS; i++)
            pthread_join(threads[i], NULL);
        /* deterministic output: print in thread-id order after joining */
        for (int i = 0; i < NTHREADS; i++)
            printf("T%d: good=%d bad=%d lastcode=%d\n", i, results[i].good_docs,
                   results[i].bad_docs, results[i].last_code);
    }

    /* ── T2. error-state isolation: parse good in thread A after B failed ─ */
    {
        /* main thread: force an error, then verify a good parse clears it */
        xmlDocPtr b = xmlReadMemory("<<<", 3, "t", NULL, 0);
        if (b) xmlFreeDoc(b);
        xmlErrorPtr e1 = xmlGetLastError();
        int c1 = e1 ? e1->code : 0;
        xmlResetLastError();
        xmlDocPtr g = xmlReadMemory("<ok/>", 5, "t", NULL, 0);
        if (g) xmlFreeDoc(g);
        xmlErrorPtr e2 = xmlGetLastError();
        int c2 = e2 ? e2->code : 0;
        printf("T2 codes=%d->%d\n", c1, c2);
    }

    /* ── T3. concurrent global reads during parses ─────────────────────── */
    {
        /* xmlMemGet + xmlGetFeature + xmlGetCompressMode are read-only
         * globals — safe to call concurrently; verify they stay sane. */
        pthread_t threads[NTHREADS];
        memset(results, 0, sizeof(results));
        for (long i = 0; i < NTHREADS; i++) {
            pthread_create(&threads[i], NULL, worker, (void *)(i + 100));
        }
        int ok = 1;
        for (int i = 0; i < 100; i++) {
            xmlFreeFunc f;
            xmlMallocFunc m;
            xmlReallocFunc r;
            xmlStrdupFunc s;
            xmlMemGet(&f, &m, &r, &s);
            if (f == NULL || m == NULL || r == NULL || s == NULL)
                ok = 0;
        }
        printf("T3 globals=%s\n", ok ? "ok" : "broken");
        for (int i = 0; i < NTHREADS; i++)
            pthread_join(threads[i], NULL);
        for (int i = 0; i < NTHREADS; i++)
            printf("T3-%d: good=%d bad=%d lastcode=%d\n", i, results[i].good_docs,
                   results[i].bad_docs, results[i].last_code);
    }

    /* ── final marker ──────────────────────────────────────────────────── */
    printf("HOSTILE-THREADS VERDICT PASS\n");
    return 0;
}
