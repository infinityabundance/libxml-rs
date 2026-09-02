/* nspush-probe.c — SP-14.3.1-3: expat-compat push parse with xmlns decls.
 * Replicates PHP ext/xml compat handler wiring at the raw libxml2 API level:
 * xmlCreatePushParserCtxt with a SAX1+SAX2 handler, xmlCtxtUseOptions
 * (XML_PARSE_OLDSAX | XML_PARSE_NOENT), then xmlParseChunk terminate. */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>

static void start_el(void *u, const xmlChar *name, const xmlChar **atts) {
    printf("S1(%s)", (const char *) name);
}
static void end_el(void *u, const xmlChar *name) {
    printf("E1(%s)", (const char *) name);
}
static void start_el_ns(void *u, const xmlChar *local, const xmlChar *pref,
                        const xmlChar *URI, int nb_ns, const xmlChar **ns,
                        int nb_att, int nb_def, const xmlChar **atts) {
    printf("S2(local=%s pref=%s uri=%s)", local ? (char*)local : "(null)",
           pref ? (char*)pref : "(null)", URI ? (char*)URI : "(null)");
}
static void end_el_ns(void *u, const xmlChar *local, const xmlChar *pref,
                      const xmlChar *URI) {
    printf("E2(local=%s pref=%s)", local ? (char*)local : "(null)",
           pref ? (char*)pref : "(null)");
}

static xmlSAXHandler handlers = {
    /* internalSubset .. externalSubset: 27 v1-prefix slots */
    NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL,
    NULL, NULL, start_el, end_el, NULL, NULL, NULL, NULL, NULL, NULL, NULL,
    NULL, NULL, NULL, NULL,
    XML_SAX2_MAGIC, /* initialized */
    NULL,           /* _private */
    start_el_ns, end_el_ns, NULL /* serror */
};

static void run(const char *label, const char *doc, int magic) {
    xmlParserCtxtPtr ctxt = xmlCreatePushParserCtxt(&handlers, NULL, NULL, 0, NULL);
    if (!ctxt) { printf("%s: no ctxt\n", label); return; }
    if (!magic) ctxt->sax->initialized = 1; /* SAX1 compat reset */
    xmlCtxtUseOptions(ctxt, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
    int rc = xmlParseChunk(ctxt, doc, (int) strlen(doc), 1);
    printf("%s: rc=%d errNo=%d wf=%d -> ", label, rc, ctxt->errNo, ctxt->wellFormed);
    printf("\n");
    xmlFreeParserCtxt(ctxt);
}

int main(void) {
    run("sax1-nsdoc", "<a xmlns=\"http://e.com/f\" xmlns:bar=\"http://e.com/b\"><bar:b foo=\"x\"/></a>", 0);
    run("sax2-nsdoc", "<a xmlns=\"http://e.com/f\" xmlns:bar=\"http://e.com/b\"><bar:b foo=\"x\"/></a>", 1);
    run("sax1-plain", "<a><bar:b foo=\"x\"/></a>", 0);
    return 0;
}
