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
        /* R-000167: xsltLibxsltVersion is a DATA symbol (const int, oracle R) */
        const int *ver = (const int *)dlsym(hx, "xsltLibxsltVersion");
        CHECK(ver != NULL, "dlsym(xsltLibxsltVersion) data");
        if (ver) {
            printf("INFO xsltLibxsltVersion=%d\n", *ver);
            CHECK(*ver == 10145, "xsltLibxsltVersion value");
        }
        const char *const *eng = (const char *const *)dlsym(hx, "xsltEngineVersion");
        CHECK(eng != NULL && *eng != NULL && (*eng)[0] != '\0', "dlsym(xsltEngineVersion) data");
        if (eng && *eng) printf("INFO xsltEngineVersion=%s\n", *eng);
        void *(*apply)(void *, void *, const char **) = dlsym(hx, "xsltApplyStylesheet");
        CHECK(apply != NULL, "dlsym(xsltApplyStylesheet)");
    }

    /* libexslt.so.0 — exsltRegisterAll reachable + version data */
    void *he = dlopen("libexslt.so.0", RTLD_NOW | RTLD_LOCAL);
    CHECK(he != NULL, "dlopen(libexslt.so.0)");
    if (he) {
        exsltRegisterAllFn reg = (exsltRegisterAllFn)dlsym(he, "exsltRegisterAll");
        CHECK(reg != NULL, "dlsym(exsltRegisterAll)");
        if (reg) reg();
        const char *const *libv = (const char *const *)dlsym(he, "exsltLibraryVersion");
        CHECK(libv != NULL && *libv != NULL, "dlsym(exsltLibraryVersion) data");
        if (libv && *libv) printf("INFO exsltLibraryVersion=%s\n", *libv);
        const int *ev = (const int *)dlsym(he, "exsltLibexsltVersion");
        CHECK(ev != NULL, "dlsym(exsltLibexsltVersion) data");
        if (ev) {
            printf("INFO exsltLibexsltVersion=%d\n", *ev);
            CHECK(*ev == 825, "exsltLibexsltVersion value");
        }
        const int *lxv = (const int *)dlsym(he, "exsltLibxsltVersion");
        CHECK(lxv != NULL && *lxv == 10145, "dlsym(exsltLibxsltVersion) data");
        const int *lmv = (const int *)dlsym(he, "exsltLibxmlVersion");
        CHECK(lmv != NULL && *lmv == 21501, "dlsym(exsltLibxmlVersion) data");
    }

    /* ── 11.1-X R-000165 closure: the newly exported surface ─────────────── */
    {
        /* xmlCtxt accessors: create a context, exercise the 2.14+ family. */
        void *(*newCtxt)(void) = (void *(*)(void))dlsym(h, "xmlNewParserCtxt");
        void *(*freeCtxt)(void *) = (void *(*)(void *))dlsym(h, "xmlFreeParserCtxt");
        int (*getOpts)(void *) = (int (*)(void *))dlsym(h, "xmlCtxtGetOptions");
        int (*setOpts)(void *, int) = (int (*)(void *, int))dlsym(h, "xmlCtxtSetOptions");
        void *(*getPriv)(void *) = (void *(*)(void *))dlsym(h, "xmlCtxtGetPrivate");
        void (*setPriv)(void *, void *) = (void (*)(void *, void *))dlsym(h, "xmlCtxtSetPrivate");
        int (*isHtml)(void *) = (int (*)(void *))dlsym(h, "xmlCtxtIsHtml");
        int (*isStopped)(void *) = (int (*)(void *))dlsym(h, "xmlCtxtIsStopped");
        int (*isInSubset)(void *) = (int (*)(void *))dlsym(h, "xmlCtxtIsInSubset");
        int (*getStatus)(void *) = (int (*)(void *))dlsym(h, "xmlCtxtGetStatus");
        void *(*getDoc)(void *) = (void *(*)(void *))dlsym(h, "xmlCtxtGetDocument");
        int (*getStandalone)(void *) = (int (*)(void *))dlsym(h, "xmlCtxtGetStandalone");
        const char *(*getVersion)(void *) = (const char *(*)(void *))dlsym(h, "xmlCtxtGetVersion");
        CHECK(getOpts && setOpts && getPriv && setPriv && isHtml && isStopped &&
              isInSubset && getStatus && getDoc && getStandalone && getVersion,
              "dlsym(xmlCtxt* accessors)");
        if (newCtxt && freeCtxt) {
            void *ctxt = newCtxt();
            if (ctxt) {
                CHECK(getOpts(ctxt) == 0, "xmlCtxtGetOptions initial");
                CHECK(setOpts(ctxt, 1) == 0, "xmlCtxtSetOptions");
                CHECK(getOpts(ctxt) == 1, "xmlCtxtGetOptions after set");
                int marker = 42;
                setPriv(ctxt, &marker);
                CHECK(getPriv(ctxt) == &marker, "xmlCtxtGet/SetPrivate");
                CHECK(isHtml(ctxt) == 0, "xmlCtxtIsHtml (XML ctxt)");
                CHECK(isStopped(ctxt) == 0, "xmlCtxtIsStopped initial");
                CHECK(isInSubset(ctxt) == 0, "xmlCtxtIsInSubset initial");
                CHECK(getStandalone(ctxt) == -1, "xmlCtxtGetStandalone tri-state");
                CHECK(getStatus(ctxt) == 0, "xmlCtxtGetStatus clean");
                CHECK(getDoc(ctxt) == NULL, "xmlCtxtGetDocument none");
                freeCtxt(ctxt);
            }
        }

        /* xmlNewInputFrom* family: build an input and read its data. */
        void *(*newFromMem)(const char *, const void *, size_t, int) =
            (void *(*)(const char *, const void *, size_t, int))dlsym(h, "xmlNewInputFromMemory");
        void *(*newFromStr)(const char *, const char *, int) =
            (void *(*)(const char *, const char *, int))dlsym(h, "xmlNewInputFromString");
        void (*freeIn)(void *) = (void (*)(void *))dlsym(h, "xmlFreeInputStream");
        CHECK(newFromMem && newFromStr && freeIn, "dlsym(xmlNewInputFrom*)");
        if (newFromMem && freeIn) {
            void *inp = newFromMem("mem.xml", "<a/>", 4, 0);
            CHECK(inp != NULL, "xmlNewInputFromMemory");
            if (inp) {
                const unsigned char **base = (const unsigned char **)inp;
                (void)base;
                freeIn(inp);
            }
        }
        if (newFromStr && freeIn) {
            void *inp = newFromStr("str.xml", "<b/>", 0);
            CHECK(inp != NULL, "xmlNewInputFromString");
            if (inp) freeIn(inp);
        }

        /* encoding conversions */
        int (*isolat1)(unsigned char *, int *, const unsigned char *, int *) =
            (int (*)(unsigned char *, int *, const unsigned char *, int *))dlsym(h, "xmlIsolat1ToUTF8");
        int (*utf8to1)(unsigned char *, int *, const unsigned char *, int *) =
            (int (*)(unsigned char *, int *, const unsigned char *, int *))dlsym(h, "xmlUTF8ToIsolat1");
        CHECK(isolat1 && utf8to1, "dlsym(encoding conversions)");
        if (isolat1) {
            unsigned char out[8];
            const unsigned char in[1] = {0xE9}; /* latin1 é */
            int olen = 8, ilen = 1;
            int n = isolat1(out, &olen, in, &ilen);
            CHECK(n == 2 && olen == 2 && out[0] == 0xC3 && out[1] == 0xA9,
                  "xmlIsolat1ToUTF8 converts");
        }

        /* xlink detection */
        int (*xlinkIsLink)(void *, void *) = (int (*)(void *, void *))dlsym(h, "xlinkIsLink");
        CHECK(xlinkIsLink != NULL, "dlsym(xlinkIsLink)");
        CHECK(xlinkIsLink(NULL, NULL) == 0, "xlinkIsLink(NULL) = NONE");
        void *(*getDetect)(void) = (void *(*)(void))dlsym(h, "xlinkGetDefaultDetect");
        CHECK(getDetect != NULL && getDetect() == NULL, "xlinkGetDefaultDetect NULL");

        /* xslt helpers — Phase 12 contract: xslt* live in the libxslt.so.1
         * facade, not the core. xsltGetUTF8CharZ is an INTERNAL_LEAK that
         * the exact export map hides (EXPORT-SURFACE-DISPOSITION) -> dlsym
         * must FAIL (negative). The oracle-exported helpers still resolve. */
        unsigned int (*utf8z)(const unsigned char *, int *) =
            (unsigned int (*)(const unsigned char *, int *))dlsym(hx, "xsltGetUTF8CharZ");
        void (*setDbgStatus)(int) = (void (*)(int))dlsym(hx, "xsltSetDebuggerStatus");
        int (*setDbgCb)(int, void *) = (int (*)(int, void *))dlsym(hx, "xsltSetDebuggerCallbacks");
        int *dbgStatus = (int *)dlsym(hx, "xslDebugStatus");
        CHECK(utf8z == NULL, "dlsym(xsltGetUTF8CharZ) hidden (INTERNAL_LEAK)");
        CHECK(setDbgStatus && setDbgCb && dbgStatus,
              "dlsym(xslt helpers)");
        if (setDbgCb) {
            CHECK(setDbgCb(3, NULL) == -1, "xsltSetDebuggerCallbacks NULL block");
        }
        if (dbgStatus) {
            printf("INFO xslDebugStatus=%d\n", *dbgStatus);
            CHECK(*dbgStatus == 0, "xslDebugStatus initial");
        }

        /* per-module EXSLT registration entry points — Phase 12 contract:
         * they live in the libexslt.so.0 facade; exsltCryptoRegister is an
         * INTERNAL_LEAK (never an oracle export) -> negative dlsym. */
        void (*reg)(void) = NULL;
        const char *mods[] = {"exsltCommonRegister", "exsltMathRegister", "exsltSetsRegister",
                              "exsltFuncRegister", "exsltStrRegister", "exsltDateRegister",
                              "exsltSaxonRegister", "exsltDynRegister"};
        int all_present = 1;
        for (size_t i = 0; i < sizeof(mods) / sizeof(mods[0]); i++) {
            if (dlsym(he, mods[i]) == NULL) all_present = 0;
        }
        CHECK(all_present, "dlsym(per-module exslt*Register)");
        CHECK(dlsym(he, "exsltCryptoRegister") == NULL,
              "dlsym(exsltCryptoRegister) hidden (INTERNAL_LEAK)");
        reg = (void (*)(void))dlsym(he, "exsltCommonRegister");
        if (reg) reg();
        int (*xreg)(void *, const char *) = (int (*)(void *, const char *))dlsym(he, "exsltDateXpathCtxtRegister");
        CHECK(xreg != NULL, "dlsym(exslt*XpathCtxtRegister)");
        if (xreg) CHECK(xreg(NULL, "date") == 0, "exsltDateXpathCtxtRegister");

        /* schematron helper surface */
        int (*setValidOpts)(void *, int) = (int (*)(void *, int))dlsym(h, "xmlSchematronSetValidOptions");
        int (*getValidOpts)(void *) = (int (*)(void *))dlsym(h, "xmlSchematronValidCtxtGetOptions");
        CHECK(setValidOpts && getValidOpts, "dlsym(xmlSchematron* options)");
    }

    dlclose(he);
    dlclose(hx);
    dlclose(h);
    printf("%s\n", failed ? "VERDICT FAIL" : "VERDICT PASS");
    return failed;
}
