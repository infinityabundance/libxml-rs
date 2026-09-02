/* chunkrc-probe.c — what does a NON-final xmlParseChunk return when the fed
 * document is complete but ends with a fatal (root end-tag mismatch)? */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>

static void s2(void *u, const xmlChar *local, const xmlChar *pref,
               const xmlChar *URI, int nb_ns, const xmlChar **ns,
               int nb_att, int nb_def, const xmlChar **atts) {
    (void) u; (void) pref; (void) URI; (void) nb_ns; (void) ns;
    (void) nb_att; (void) nb_def; (void) atts;
    printf("S2(%s) ", local ? (char *) local : "(null)");
}

int main(void) {
    xmlSAXHandler h; memset(&h, 0, sizeof(h));
    h.initialized = XML_SAX2_MAGIC;
    h.startElementNs = s2;
    xmlParserCtxtPtr c = xmlCreatePushParserCtxt(&h, NULL, NULL, 0, NULL);
    xmlCtxtUseOptions(c, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
    const char *doc = "<foo:a xmlns:foo=\"u\"><bar:b xmlns:bar=\"v\"/></foo>";
    int r = xmlParseChunk(c, doc, (int) strlen(doc), 0);
    printf("\nnon-final rc=%d err=%d wf=%d\n", r, c->errNo, c->wellFormed);
    int r2 = xmlParseChunk(c, NULL, 0, 1);
    printf("final    rc=%d err=%d wf=%d\n", r2, c->errNo, c->wellFormed);
    xmlFreeParserCtxt(c);
    return 0;
}
