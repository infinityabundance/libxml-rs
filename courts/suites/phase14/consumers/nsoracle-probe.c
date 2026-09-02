/* nsoracle-probe.c — run against the ORACLE (system libxml2) to pin what the
 * parser dispatches for PHP expat-compat handler configurations. */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>

static void s1(void *u, const xmlChar *name, const xmlChar **atts) {
    printf("S1[name=%s atts=", (const char *) name);
    if (atts) for (int i = 0; atts[i]; i += 2) printf("%s=%s,", (char*)atts[i], (char*)atts[i+1]);
    printf("]");
}
static void e1(void *u, const xmlChar *name) {
    printf("E1[%s]", (const char *) name);
}
static void s2(void *u, const xmlChar *local, const xmlChar *pref,
               const xmlChar *URI, int nb_ns, const xmlChar **ns,
               int nb_att, int nb_def, const xmlChar **atts) {
    printf("S2[local=%s pref=%s uri=%s ns=", local ? (char*)local : "(null)",
           pref ? (char*)pref : "(null)", URI ? (char*)URI : "(null)");
    for (int i = 0; i < nb_ns; i++) printf("%s=%s,", ns[2*i] ? (char*)ns[2*i] : "(null)", (char*)ns[2*i+1]);
    printf(" atts=");
    for (int i = 0; i < nb_att; i++) {
        printf("%s(uri=%s val=%.*s),", (char*)atts[5*i],
               atts[5*i+2] ? (char*)atts[5*i+2] : "(null)",
               (int)(atts[5*i+4]-atts[5*i+3]), (char*)atts[5*i+3]);
    }
    printf("]");
}
static void e2(void *u, const xmlChar *local, const xmlChar *pref, const xmlChar *URI) {
    printf("E2[%s pref=%s]", (char*)local, pref ? (char*)pref : "(null)");
}

static void run(const char *label, const char *doc, int ns_mode, int sax1_flag) {
    xmlSAXHandler h; memset(&h, 0, sizeof(h));
    h.startElement = s1; h.endElement = e1;
    h.initialized = XML_SAX2_MAGIC;
    h.startElementNs = s2; h.endElementNs = e2;
    xmlParserCtxtPtr ctxt = xmlCreatePushParserCtxt(&h, NULL, NULL, 0, NULL);
    if (!ctxt) { printf("%s: no ctxt\n", label); return; }
    xmlCtxtUseOptions(ctxt, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
    if (!ns_mode && sax1_flag) ctxt->sax->initialized = 1; /* php non-NS reset */
    int rc = xmlParseChunk(ctxt, doc, (int) strlen(doc), 1);
    printf("%s: rc=%d errNo=%d -> ", label, rc, ctxt->errNo);
    printf("\n");
    xmlFreeParserCtxt(ctxt);
}

int main(void) {
    const char *nsdoc = "<ns1:listOfAwards xmlns:ns1=\"http://www.fpdsng.com/FPDS\">"
                        "<ns1:count><ns1:total>867</ns1:total></ns1:count></ns1:listOfAwards>";
    const char *plain = "<a xmlns=\"http://e.com/f\" xmlns:bar=\"http://e.com/b\"><bar:b foo=\"x\"/></a>";
    run("ns1doc sax1flag(php-non-ns)", nsdoc, 0, 1);
    run("ns1doc noflag (php-ns)      ", nsdoc, 1, 0);
    run("plain  sax1flag(php-non-ns) ", plain, 0, 1);
    return 0;
}
