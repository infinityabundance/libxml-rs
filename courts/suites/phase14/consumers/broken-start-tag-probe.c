#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/parserInternals.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

/* not_well_formed2.xml (php ext/dom load_error2_gte2_12): an unquoted
 * attribute value breaks the <book number=nine> start tag. Where does the
 * oracle resume scanning, and what SAX events fire before the stray </book>
 * at line 7 closes <books> (mismatch, warning 4)? */

static const char DOC[] =
    "<?xml version=\"1.0\" ?>\n"
    "<!-- AttValue: \" or ' expected -->\n"
    "<books>\n"
    " <book number=nine>\n"
    "  <title>The Grapes of Wrath</title>\n"
    "  <author>John Steinbeck</author>\n"
    " </book>\n"
    "</books>\n";

static int depth = 0;

static void start_el(void *u, const xmlChar *name, const xmlChar **atts) {
    (void) u; (void) atts;
    printf("+%*s%s\n", depth * 2, "", (const char *) name);
    depth++;
}
static void end_el(void *u, const xmlChar *name) {
    (void) u;
    if (depth > 0) depth--;
    printf("-%*s%s\n", depth * 2, "", (const char *) name);
}
static void chars(void *u, const xmlChar *ch, int len) {
    (void) u;
    printf("  text[%d]=%.*s\n", len, len, (const char *) ch);
}
static void start_el_ns(void *u, const xmlChar *localname, const xmlChar *prefix,
                        const xmlChar *URI, int nb_namespaces,
                        const xmlChar **namespaces, int nb_attributes,
                        int nb_defaulted, const xmlChar **attributes) {
    (void) u; (void) prefix; (void) URI; (void) nb_namespaces;
    (void) namespaces; (void) nb_attributes; (void) nb_defaulted; (void) attributes;
    printf("+%*s%s\n", depth * 2, "", (const char *) localname);
    depth++;
}
static void end_el_ns(void *u, const xmlChar *localname, const xmlChar *prefix,
                      const xmlChar *URI) {
    (void) u; (void) prefix; (void) URI;
    if (depth > 0) depth--;
    printf("-%*s%s\n", depth * 2, "", (const char *) localname);
}
static void errfunc(void *u, const xmlError *e) {
    (void) u;
    printf("ERR code=%d level=%d line=%d msg=%s", e->code, e->level, e->line,
           e->message ? (const char *) e->message : "(null)\n");
}

int main(void) {
    xmlSAXHandler *h = xmlMalloc(sizeof(*h));
    if (h == NULL) return 1;
    memset(h, 0, sizeof(*h));
    h->startElement = start_el;
    h->endElement = end_el;
    h->characters = chars;
    h->startElementNs = start_el_ns;
    h->endElementNs = end_el_ns;
    h->initialized = XML_SAX2_MAGIC;

    xmlSetStructuredErrorFunc(NULL, errfunc);
    xmlParserCtxtPtr ctxt = xmlCreateMemoryParserCtxt(DOC, (int) strlen(DOC));
    if (ctxt == NULL) { printf("no ctxt\n"); return 1; }
    ctxt->sax = h;
    ctxt->userData = NULL;
    xmlParseDocument(ctxt);
    printf("wellFormed=%d errNo=%d doc=%s\n", ctxt->wellFormed, ctxt->errNo,
           ctxt->myDoc ? "parsed" : "NULL");
    if (ctxt->myDoc) xmlFreeDoc(ctxt->myDoc);
    xmlFreeParserCtxt(ctxt);
    xmlSetStructuredErrorFunc(NULL, NULL);
    return 0;
}
