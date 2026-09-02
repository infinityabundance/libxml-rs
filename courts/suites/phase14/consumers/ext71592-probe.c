/* ext71592-probe.c — engine-layer probe for the bug71592 flow.
 *
 * Mirrors PHP ext/xml compat.c get_entity: on an entity reference outside the
 * DTD, resolve via xmlGetPredefinedEntity then xmlGetDocEntity(myDoc, name),
 * and report what the reference machinery did with a declared external
 * general parsed entity. userData intentionally != ctxt (PHP compat passes its
 * own parser struct), so xmlSAX2GetEntity's ctxt fallback must NOT apply.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>
#include <libxml/tree.h>

static int ge_calls = 0;
static int ext_fire = 0;

static xmlEntityPtr ge(void *u, const xmlChar *name) {
    xmlParserCtxtPtr c = (xmlParserCtxtPtr) u;
    xmlEntityPtr ret;
    ge_calls++;
    ret = xmlGetPredefinedEntity(name);
    if (ret == NULL)
        ret = xmlGetDocEntity(c->myDoc, name);
    fprintf(stderr, "getEntity(%s) myDoc=%p inSubset=%d instate=%d -> %s\n",
            (const char *) name, (void *) c->myDoc, c->inSubset, c->instate,
            ret ? (char *) ret->name : "(null)");
    if (ret != NULL)
        fprintf(stderr, "  etype=%d SystemID=%s ExternalID=%s\n",
                ret->etype,
                ret->SystemID ? (const char *) ret->SystemID : "(null)",
                ret->ExternalID ? (const char *) ret->ExternalID : "(null)");
    return ret;
}

static void s2(void *u, const xmlChar *local, const xmlChar *pref,
               const xmlChar *URI, int nb_ns, const xmlChar **ns,
               int nb_att, int nb_def, const xmlChar **atts) {
    (void) u; (void) pref; (void) URI; (void) nb_ns; (void) ns;
    (void) nb_att; (void) nb_def; (void) atts;
    printf("S2(%s) ", local ? (char *) local : "(null)");
}

static void e2(void *u, const xmlChar *local, const xmlChar *pref,
               const xmlChar *URI) {
    (void) u; (void) pref; (void) URI;
    printf("E2(%s) ", local ? (char *) local : "(null)");
}

static void chars(void *u, const xmlChar *ch, int len) {
    (void) u;
    printf("CH(%.*s) ", len, (const char *) ch);
}

int main(void) {
    xmlSAXHandler h; memset(&h, 0, sizeof(h));
    h.getEntity = ge;
    h.initialized = XML_SAX2_MAGIC;
    h.startElementNs = s2;
    h.endElementNs = e2;
    h.characters = chars;
    xmlParserCtxtPtr c = xmlCreatePushParserCtxt(&h, NULL, NULL, 0, NULL);
    xmlCtxtUseOptions(c, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
    const char *doc =
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        "<!DOCTYPE root [\n"
        "  <!ENTITY pic PUBLIC \"image.gif\" \"http://example.org/image.gif\">\n"
        "]>\n"
        "<root>\n<p>&pic;</p>\n<p></nop>\n</root>\n";
    int r = xmlParseChunk(c, doc, (int) strlen(doc), 0);
    printf("\nrc=%d err=%d wf=%d ge_calls=%d myDoc=%p intSubset=%p\n",
           r, c->errNo, c->wellFormed, ge_calls, (void *) c->myDoc,
           (void *) (c->myDoc ? c->myDoc->intSubset : NULL));
    xmlFreeParserCtxt(c);
    return 0;
}
