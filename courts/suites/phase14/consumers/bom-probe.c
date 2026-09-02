/* bom-probe.c — how do push parses handle a REAL UTF-8 BOM with/without an
 * XML declaration, and an unterminated-decl (bug35447 phpt shape)? */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>
#include <libxml/tree.h>

static void s2(void *u, const xmlChar *local, const xmlChar *pref,
               const xmlChar *URI, int nb_ns, const xmlChar **ns,
               int nb_att, int nb_def, const xmlChar **atts) {
    (void) u; (void) pref; (void) URI; (void) nb_ns; (void) ns;
    printf("S2(%s) att=%d def=%d ", local ? (char *) local : "(null)", nb_att, nb_def);
    if (atts && nb_att > 0) {
        for (int i = 0; i < nb_att; i++) {
            printf("[%s=%s]", (const char *) atts[i * 5],
                   (const char *) atts[i * 5 + 3]);
        }
    }
}

static void e2(void *u, const xmlChar *local, const xmlChar *pref,
               const xmlChar *URI) {
    (void) u; (void) pref; (void) URI;
    printf("E2(%s) ", local ? (char *) local : "(null)");
}

static void run(xmlSAXHandler *h, const char *name, const char *doc, int term) {
    xmlParserCtxtPtr c = xmlCreatePushParserCtxt(h, NULL, NULL, 0, NULL);
    xmlCtxtUseOptions(c, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
    printf("-- %s\n", name);
    int r = xmlParseChunk(c, doc, (int) strlen(doc), term);
    printf("rc=%d err=%d wf=%d\n", r, c->errNo, c->wellFormed);
    xmlFreeParserCtxt(c);
}

int main(void) {
    xmlSAXHandler h; memset(&h, 0, sizeof(h));
    h.initialized = XML_SAX2_MAGIC;
    h.startElementNs = s2;
    h.endElementNs = e2;
    const char *bom_decl = "\xEF\xBB\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?>\n"
        "<root/>\n";
    const char *bom_nodecl = "\xEF\xBB\xBF<root/>\n";
    const char *lit = "\\xEF\\xBB\\xBF<?xml version=\"1.0\" encoding=\"utf-8\"?\\x3e\n"
        "<root/>\n";
    run(&h, "BOM+xmldecl+root (final)", bom_decl, 1);
    run(&h, "BOM+root (final)", bom_nodecl, 1);
    run(&h, "literal \\xEF text phpt-shape (final)", lit, 1);
    run(&h, "BOM+xmldecl+root (non-final)", bom_decl, 0);
    return 0;
}
