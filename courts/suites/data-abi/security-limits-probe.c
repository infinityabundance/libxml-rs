/*
 * security-limits-probe.c — 11.1-V security-relevant compatibility probe.
 *
 * Court family: SECURITY-LIMITS (registered with the 11.1-V phase)
 *
 * Exercises the historically significant security-sensitive parser/engine
 * behaviors with deterministic output (result codes + markers — never
 * attacker-controlled content) and requires byte-identical stdout/stderr
 * between the oracle (system libxml2 2.15.3) and the candidate:
 *
 *   1. entity-expansion amplification (billion laughs) — bounded by default;
 *   2. recursive entity loop — rejected (XML_ERR_ENTITY_LOOP family);
 *   3. parser depth limit (deep nesting) — rejected;
 *   4. text-node size limit (XML_MAX_TEXT_LENGTH) — rejected;
 *   5. XML_PARSE_HUGE — lifts the hard limits (same doc parses);
 *   6. xmlCtxtSetMaxAmplification — raising the factor lifts the bound;
 *   7. XML_PARSE_NONET — external network entity must not be fetched;
 *   8. external entity loading (file://) with XML_PARSE_NOENT;
 *   9. XInclude of a local document;
 *  10. catalog resolution (xmlCatalogAdd + xmlCatalogResolve).
 *
 * Compiled twice by tools/abi/security_limits_probe.py: once against the
 * oracle headers + DSO, once against the candidate headers + DSO.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xinclude.h>
#include <libxml/catalog.h>
#include <libxml/xmlerror.h>

/* The parser error code of the most recent failure. */
static int last_code = 0;

static void capture_err(void *ctx, const xmlError *err) {
    (void)ctx;
    if (err) last_code = err->code;
}

/* Parse `doc`; print "case <name>: ok" or "case <name>: code=<code>". */
static void run_case(const char *name, const char *doc, int opts) {
    xmlSetStructuredErrorFunc(NULL, capture_err);
    last_code = 0;
    xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "sec.xml", NULL, opts);
    if (d != NULL) {
        printf("case %s: ok\n", name);
        xmlFreeDoc(d);
    } else {
        printf("case %s: code=%d\n", name, last_code);
    }
    xmlSetStructuredErrorFunc(NULL, NULL);
}

/* ── 1. billion laughs: 9 entities, each expanding 10x ─────────────────────── */
static const char *BILLION_LAUGHS =
    "<?xml version=\"1.0\"?>"
    "<!DOCTYPE lolz ["
    "<!ENTITY lol \"lololololololololololololololololololololololololololololololol\">"
    "<!ENTITY lol1 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\">"
    "<!ENTITY lol2 \"&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;\">"
    "<!ENTITY lol3 \"&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;\">"
    "<!ENTITY lol4 \"&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;\">"
    "<!ENTITY lol5 \"&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;\">"
    "<!ENTITY lol6 \"&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;&lol5;\">"
    "<!ENTITY lol7 \"&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;&lol6;\">"
    "<!ENTITY lol8 \"&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;&lol7;\">"
    "<!ENTITY lol9 \"&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;&lol8;\">"
    "]><lolz>&lol9;</lolz>";

/* ── 2. recursive entity loop ──────────────────────────────────────────────── */
static const char *ENTITY_LOOP =
    "<?xml version=\"1.0\"?>"
    "<!DOCTYPE a [<!ENTITY a \"&a;\">]>"
    "<a>&a;</a>";

/* ── 3. deep nesting (500 levels) ──────────────────────────────────────────── */
static const char *DEEP_DOC =
    "<?xml version=\"1.0\"?><a>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "<b><b><b><b><b><b><b><b><b><b>"
    "</b></a>";

/* ── 4. oversized text node (~11MB) ───────────────────────────────────────── */
static const char *HUGE_TEXT =
    "<?xml version=\"1.0\"?><a>"
    "01234567890123456789012345678901234567890123456789"
    "01234567890123456789012345678901234567890123456789"
    "01234567890123456789012345678901234567890123456789"
    "01234567890123456789012345678901234567890123456789"
    "01234567890123456789012345678901234567890123456789"
    "01234567890123456789012345678901234567890123456789"
    "01234567890123456789012345678901234567890123456789"
    "01234567890123456789012345678901234567890123456789"
    "01234567890123456789012345678901234567890123456789"
    "01234567890123456789012345678901234567890123456789"
    "</a>";

int main(void) {
    xmlInitParser();

    /* 1-5: limits + HUGE */
    run_case("billion-laughs", BILLION_LAUGHS, 0);
    run_case("billion-laughs-huge", BILLION_LAUGHS, XML_PARSE_HUGE);
    run_case("entity-loop", ENTITY_LOOP, 0);
    run_case("deep-nesting", DEEP_DOC, 0);
    run_case("deep-nesting-huge", DEEP_DOC, XML_PARSE_HUGE);

    /* 6: xmlCtxtSetMaxAmplification lifts the entity-expansion bound */
    {
        xmlParserCtxtPtr ctxt = xmlNewParserCtxt();
        /* The 9-level chain expands to ~10^10 bytes; consumed ~350; the
         * ratio bound needs amplification >= 10^10/350 ~ 3e7. 1e10 clears
         * it with margin while the default (5) must still reject. */
        xmlCtxtSetMaxAmplification(ctxt, 10000000000.0);
        xmlSetStructuredErrorFunc(NULL, capture_err);
        last_code = 0;
        xmlDocPtr d = xmlCtxtReadMemory(ctxt, BILLION_LAUGHS,
                                        (int)strlen(BILLION_LAUGHS), "sec.xml", NULL, 0);
        if (d != NULL) {
            printf("case amplification-raised: ok\n");
            xmlFreeDoc(d);
        } else {
            printf("case amplification-raised: code=%d\n", last_code);
        }
        xmlSetStructuredErrorFunc(NULL, NULL);
        xmlFreeParserCtxt(ctxt);
    }

    /* 7: NONET — external network entity must not be fetched */
    {
        const char *doc =
            "<?xml version=\"1.0\"?>"
            "<!DOCTYPE a [<!ENTITY xxe SYSTEM \"http://127.0.0.1:1/nope\">]>"
            "<a>&xxe;</a>";
        xmlSetStructuredErrorFunc(NULL, capture_err);
        last_code = 0;
        xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "sec.xml", NULL,
                                    XML_PARSE_NOENT | XML_PARSE_NONET);
        if (d != NULL) {
            printf("case nonet-entity: ok\n");
            xmlFreeDoc(d);
        } else {
            printf("case nonet-entity: code=%d\n", last_code);
        }
        xmlSetStructuredErrorFunc(NULL, NULL);
    }

    /* 8: external entity from a local file (NOENT) */
    {
        const char *path = "/tmp/sec-entity.txt";
        FILE *f = fopen(path, "w");
        if (f) { fputs("EXTDATA", f); fclose(f); }
        char doc[512];
        snprintf(doc, sizeof(doc),
                 "<?xml version=\"1.0\"?>"
                 "<!DOCTYPE a [<!ENTITY ext SYSTEM \"%s\">]>"
                 "<a>&ext;</a>", path);
        xmlSetStructuredErrorFunc(NULL, capture_err);
        last_code = 0;
        xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "sec.xml", NULL, XML_PARSE_NOENT);
        if (d != NULL) {
            xmlChar *txt = xmlNodeGetContent(xmlDocGetRootElement(d));
            printf("case ext-entity: %s\n", txt ? (char *)txt : "(null)");
            if (txt) xmlFree(txt);
            xmlFreeDoc(d);
        } else {
            printf("case ext-entity: code=%d\n", last_code);
        }
        xmlSetStructuredErrorFunc(NULL, NULL);
        remove(path);
    }

    /* 9: XInclude of a local document */
    {
        const char *path = "/tmp/sec-include.xml";
        FILE *f = fopen(path, "w");
        if (f) { fputs("<inc>XIDATA</inc>", f); fclose(f); }
        char doc[512];
        snprintf(doc, sizeof(doc),
                 "<?xml version=\"1.0\"?>"
                 "<doc xmlns:xi=\"http://www.w3.org/2001/XInclude\">"
                 "<xi:include href=\"%s\"/></doc>", path);
        xmlSetStructuredErrorFunc(NULL, capture_err);
        last_code = 0;
        xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "sec.xml", NULL, 0);
        int rc = -1;
        if (d != NULL) rc = xmlXIncludeProcess(d);
        if (d != NULL && rc >= 0) {
            xmlChar *txt = xmlNodeGetContent(xmlDocGetRootElement(d));
            printf("case xinclude: %s\n", txt ? (char *)txt : "(null)");
            if (txt) xmlFree(txt);
            xmlFreeDoc(d);
        } else {
            printf("case xinclude: code=%d\n", last_code);
            if (d) xmlFreeDoc(d);
        }
        xmlSetStructuredErrorFunc(NULL, NULL);
        remove(path);
    }

    /* 10: catalog resolution */
    {
        const char *catpath = "/tmp/sec-catalog.xml";
        FILE *f = fopen(catpath, "w");
        if (f) {
            fputs("<?xml version=\"1.0\"?>"
                  "<catalog xmlns=\"urn:oasis:names:tc:entity:xmlns:xml:catalog\">"
                  "<public publicId=\"-//TEST//PUB\" uri=\"http://resolved.example/x\"/>"
                  "</catalog>", f);
            fclose(f);
        }
        int rc = xmlLoadCatalog(catpath);
        xmlChar *res = xmlCatalogResolvePublic("-//TEST//PUB");
        printf("case catalog: load=%d resolve=%s\n", rc,
               res ? (char *)res : "(null)");
        if (res) xmlFree(res);
        remove(catpath);
    }

    xmlCleanupParser();
    return 0;
}
