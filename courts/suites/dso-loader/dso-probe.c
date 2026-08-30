/*
 * dso-probe.c — 11.1-T dynamic-loader court probe.
 *
 * Court family: DSO-LOADER
 *
 * Treats the candidate as an opaque shared object, exactly as a drop-in
 * consumer would:
 *
 *   1. dlopen() the three upstream runtime names (libxml2.so.16,
 *      libxslt.so.1, libexslt.so.0) through the SONAME chains;
 *   2. dlsym() and CALL functions (parser, tree, XSLT, EXSLT);
 *   3. dlsym() and READ data globals (xmlParserVersion);
 *   4. install a callback (xmlSetStructuredErrorFunc) and verify it fires;
 *   5. prove CANDIDATE IDENTITY: dladdr() on a resolved symbol must report
 *      a path inside the artifact directory — NOT a system libxml2.
 *
 * Compiled with only the standard toolchain against the installed headers.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

/* Runtime API surface (opaque — resolved via dlsym) */
typedef void *(*xmlReadMemoryFn)(const char *, int, const char *, const char *, int);
typedef void (*xmlFreeDocFn)(void *);
typedef void (*xmlSetStructuredErrorFuncFn)(void *, void (*)(void *, void *));
typedef int (*xsltLibxsltVersionNumFn)(void);
typedef void (*exsltRegisterAllFn)(void);

static int errors_seen = 0;
static void err_cb(void *ctx, void *err) { (void)ctx; (void)err; errors_seen++; }

static int failed = 0;
#define CHECK(cond, msg) do { \
    if (cond) { printf("PASS %s\n", msg); } \
    else { printf("FAIL %s\n", msg); failed = 1; } \
} while (0)

int main(int argc, char **argv) {
    const char *artifact = (argc > 1) ? argv[1] : "";
    void *h;

    /* 1. dlopen the upstream runtime name via the SONAME chain */
    h = dlopen("libxml2.so.16", RTLD_NOW | RTLD_LOCAL);
    CHECK(h != NULL, "dlopen(libxml2.so.16)");
    if (!h) { printf("dlerror: %s\n", dlerror()); return 1; }

    /* 2. dlsym + call functions */
    xmlReadMemoryFn xmlReadMemory = (xmlReadMemoryFn)dlsym(h, "xmlReadMemory");
    xmlFreeDocFn xmlFreeDoc = (xmlFreeDocFn)dlsym(h, "xmlFreeDoc");
    CHECK(xmlReadMemory != NULL && xmlFreeDoc != NULL, "dlsym(xmlReadMemory/xmlFreeDoc)");
    if (xmlReadMemory && xmlFreeDoc) {
        void *doc = xmlReadMemory("<root><child/></root>", 20, "t.xml", NULL, 0);
        CHECK(doc != NULL, "xmlReadMemory parses");
        xmlFreeDoc(doc);
    }

    /* 3. data globals */
    const char *const *pver = (const char *const *)dlsym(h, "xmlParserVersion");
    CHECK(pver != NULL && *pver != NULL && (*pver)[0] != '\0', "dlsym(xmlParserVersion) data");
    if (pver && *pver) printf("INFO xmlParserVersion=%s\n", *pver);

    /* 4. callback registration + firing */
    xmlSetStructuredErrorFuncFn setErr =
        (xmlSetStructuredErrorFuncFn)dlsym(h, "xmlSetStructuredErrorFunc");
    CHECK(setErr != NULL, "dlsym(xmlSetStructuredErrorFunc)");
    if (setErr) {
        setErr(NULL, err_cb);
        xmlReadMemoryFn rm = (xmlReadMemoryFn)dlsym(h, "xmlReadMemory");
        if (rm) {
            void *d = rm("<root>", 6, "t.xml", NULL, 0);
            if (d) xmlFreeDoc(d);
        }
        CHECK(errors_seen > 0, "structured-error callback fired");
        setErr(NULL, NULL);
    }

    /* 5. candidate identity — the loaded object must be OURS */
    {
        Dl_info info;
        void *sym = dlsym(h, "xmlReadMemory");
        if (dladdr(sym, &info) && info.dli_fname) {
            printf("INFO loaded-object=%s\n", info.dli_fname);
            if (artifact[0] != '\0' && strstr(info.dli_fname, artifact) != NULL) {
                CHECK(1, "candidate identity (loaded object inside artifact)");
            } else {
                CHECK(0, "candidate identity (loaded object inside artifact)");
            }
        } else {
            CHECK(0, "dladdr on resolved symbol");
        }
    }

    /* libxslt.so.1 — same combined DSO, xslt surface reachable */
    void *hx = dlopen("libxslt.so.1", RTLD_NOW | RTLD_LOCAL);
    CHECK(hx != NULL, "dlopen(libxslt.so.1)");
    if (hx) {
        xsltLibxsltVersionNumFn ver = (xsltLibxsltVersionNumFn)dlsym(hx, "xsltLibxsltVersion");
        CHECK(ver != NULL, "dlsym(xsltLibxsltVersion)");
        void *(*apply)(void *, void *, const char **) = dlsym(hx, "xsltApplyStylesheet");
        CHECK(apply != NULL, "dlsym(xsltApplyStylesheet)");
    }

    /* libexslt.so.0 — exsltRegisterAll reachable */
    void *he = dlopen("libexslt.so.0", RTLD_NOW | RTLD_LOCAL);
    CHECK(he != NULL, "dlopen(libexslt.so.0)");
    if (he) {
        exsltRegisterAllFn reg = (exsltRegisterAllFn)dlsym(he, "exsltRegisterAll");
        CHECK(reg != NULL, "dlsym(exsltRegisterAll)");
        if (reg) reg();
    }

    dlclose(he);
    dlclose(hx);
    dlclose(h);
    printf("%s\n", failed ? "VERDICT FAIL" : "VERDICT PASS");
    return failed;
}
