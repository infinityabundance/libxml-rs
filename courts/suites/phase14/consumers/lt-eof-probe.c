/* lt-eof-probe.c — how does the oracle treat a chunk ending in '<' (non-final
 * and final), and the per-char default-handler feed? */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>

static void chars(void *u, const xmlChar *ch, int len) {
    (void) u;
    printf("CH(%.*s) ", len, (const char *) ch);
}
static void s1(void *u, const xmlChar *name, const xmlChar **atts) {
    (void) u; (void) atts;
    printf("S1(%s) ", name ? (char *) name : "(null)");
}
static void e1(void *u, const xmlChar *name) {
    (void) u;
    printf("E1(%s) ", name ? (char *) name : "(null)");
}

int main(void) {
    xmlSAXHandler h; memset(&h, 0, sizeof(h));
    h.initialized = 1;
    h.characters = chars;
    h.startElement = s1;
    h.endElement = e1;
    xmlParserCtxtPtr c = xmlCreatePushParserCtxt(&h, NULL, NULL, 0, NULL);
    /* feed "<" non-final, then the rest */
    int r1 = xmlParseChunk(c, "<", 1, 0);
    printf("after '<' non-final: rc=%d err=%d wf=%d\n", r1, c->errNo, c->wellFormed);
    const char *rest = "!-- xxx --><foo attr1=\"&quot;\"></foo>";
    int r2 = xmlParseChunk(c, rest, (int) strlen(rest), 1);
    printf("after rest final: rc=%d err=%d wf=%d\n", r2, c->errNo, c->wellFormed);
    xmlFreeParserCtxt(c);

    /* per-char feed */
    printf("-- per-char --\n");
    xmlSAXHandler h2; memset(&h2, 0, sizeof(h2));
    h2.initialized = XML_SAX2_MAGIC;
    h2.startElementNs = (void *) s1;
    xmlParserCtxtPtr c2 = xmlCreatePushParserCtxt(&h2, NULL, NULL, 0, NULL);
    xmlCtxtUseOptions(c2, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
    const char *doc = "<!-- xxx --><foo attr1=\"&quot;\"></foo>";
    int rc_all = 0;
    for (size_t i = 0; i < strlen(doc); i++) {
        int r = xmlParseChunk(c2, doc + i, 1, 0);
        if (r != 0) { printf("char %zu rc=%d err=%d\n", i, r, c2->errNo); rc_all++; }
    }
    int rf = xmlParseChunk(c2, "", 0, 1);
    printf("per-char non-final failures=%d final rc=%d err=%d wf=%d\n", rc_all, rf, c2->errNo, c2->wellFormed);
    xmlFreeParserCtxt(c2);
    return 0;
}
