/* extget-probe.c — does the engine invoke the SAX getEntity callback for a
 * declared external general parsed entity in content (bug71592 flow)? */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>
#include <libxml/tree.h>

static xmlEntityPtr ge(void *u, const xmlChar *name) {
    fprintf(stderr, "getEntity(%s)\n", (const char *) name);
    return NULL;
}

static void s2(void *u, const xmlChar *local, const xmlChar *pref,
               const xmlChar *URI, int nb_ns, const xmlChar **ns,
               int nb_att, int nb_def, const xmlChar **atts) {
    printf("S2(%s uri=%s) ", local ? (char *) local : "(null)", URI ? (char *) URI : "(null)");
}

int main(void) {
    xmlSAXHandler h; memset(&h, 0, sizeof(h));
    h.getEntity = ge;
    h.initialized = XML_SAX2_MAGIC;
    h.startElementNs = s2;
    xmlParserCtxtPtr c = xmlCreatePushParserCtxt(&h, NULL, NULL, 0, NULL);
    xmlCtxtUseOptions(c, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
    const char *doc =
        "<!DOCTYPE root [\n"
        "  <!ENTITY pic PUBLIC \"image.gif\" \"http://example.org/image.gif\">\n"
        "]>\n"
        "<root><p>&pic;</p><q>after</q></root>";
    int r = xmlParseChunk(c, doc, (int) strlen(doc), 0);
    printf("\nrc=%d err=%d wf=%d\n", r, c->errNo, c->wellFormed);
    xmlFreeParserCtxt(c);
    return 0;
}
