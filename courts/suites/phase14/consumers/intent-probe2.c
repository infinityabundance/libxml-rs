/* intent-probe2.c — byte-exact php expat-compat clone for the internal-entity
 * flow (gh14834/bug30875): full compat get_entity branch logic + cdata/default
 * feeds replicated, chars delivered through sax.characters, instate logged.
 * Run against oracle AND candidate; the delivery pattern must match. */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/SAX2.h>
#include <libxml/tree.h>

static int ge_calls = 0;
static int h_cdata_feed = 0;
static int h_default_feed = 0;
static xmlParserCtxtPtr G_CTXT;
static xmlChar *(*G_dup)(const xmlChar *) = NULL;

/* compat.c build_entity */
static void build_entity(const xmlChar *name, size_t len, xmlChar **entity, size_t *entity_len) {
    *entity_len = len + 2;
    *entity = xmlMalloc(*entity_len + 1);
    memcpy(*entity, "&", 1);
    memcpy(*entity + 1, name, len);
    (*entity)[len + 1] = ';';
    (*entity)[*entity_len] = '\0';
}

static int h_default_present = 0;
static int h_cdata_present = 1;

/* compat.c get_entity replicated */
static xmlEntityPtr ge(void *u, const xmlChar *name) {
    (void) u;
    xmlParserCtxtPtr c = G_CTXT;
    xmlEntityPtr ret = NULL;
    ge_calls++;
    if (c->inSubset == 0) {
        ret = xmlGetPredefinedEntity(name);
        if (ret == NULL)
            ret = xmlGetDocEntity(c->myDoc, name);
        fprintf(stderr, "getEntity(%s) instate=%d CONTENT=%d ret=%s etype=%d\n",
                (const char *) name, c->instate, XML_PARSER_CONTENT,
                ret ? (char *) ret->name : "(null)", ret ? ret->etype : -1);
        if (ret == NULL || c->instate == XML_PARSER_CONTENT) {
            if (ret == NULL || ret->etype == XML_INTERNAL_GENERAL_ENTITY ||
                ret->etype == XML_INTERNAL_PARAMETER_ENTITY ||
                ret->etype == XML_INTERNAL_PREDEFINED_ENTITY) {
                /* internal branch */
                int is_predef = (ret && ret->etype == XML_INTERNAL_PREDEFINED_ENTITY);
                if (h_default_present && !(ret && is_predef && h_cdata_present)) {
                    xmlChar *entity; size_t len;
                    build_entity(name, xmlStrlen(name), &entity, &len);
                    h_default_feed++;
                    fprintf(stderr, "  -> h_default feed (&%s;)\n", (char *) name);
                    xmlFree(entity);
                } else {
                    if (h_cdata_present && ret) {
                        h_cdata_feed++;
                        fprintf(stderr, "  -> h_cdata feed (%s)\n", (char *) ret->content);
                    }
                }
            } else {
                if (ret->etype == XML_EXTERNAL_GENERAL_PARSED_ENTITY) {
                    fprintf(stderr, "  -> external-entity-ref handler\n");
                }
            }
        }
    }
    return ret;
}

static void chars(void *u, const xmlChar *ch, int len) {
    (void) u;
    printf("CH(%.*s) ", len, (const char *) ch);
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

int main(int argc, char **argv) {
    int with_default = argc > 1;
    h_default_present = with_default;
    xmlSAXHandler h; memset(&h, 0, sizeof(h));
    h.initialized = XML_SAX2_MAGIC;
    h.getEntity = ge;
    h.characters = chars;
    h.startElementNs = s2;
    h.endElementNs = e2;
    char ud;
    xmlParserCtxtPtr c = xmlCreatePushParserCtxt(&h, &ud, NULL, 0, NULL);
    G_CTXT = c;
    xmlCtxtUseOptions(c, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
    c->wellFormed = 0;
    const char *doc =
        "<!DOCTYPE root [\n"
        "  <!ENTITY foo \"ent\">\n"
        "]>\n"
        "<root>\n<element>&foo;</element>\n</root>\n";
    int r = xmlParseChunk(c, doc, (int) strlen(doc), 0);
    printf("\nrc=%d err=%d wf=%d ge_calls=%d cdata_feed=%d default_feed=%d\n",
           r, c->errNo, c->wellFormed, ge_calls, h_cdata_feed, h_default_feed);
    xmlFreeParserCtxt(c);
    return 0;
}
