/* entprobe2.c — does content after an expanded general entity survive in a
 * SAX push parse? Candidate drops elem4 (xml004) after &included-entity; inside
 * elem3. Pure-libxml probe comparing system libxml2 (oracle) vs candidate. */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

static xmlParserCtxtPtr GCTX;
static xmlEntityPtr ge(void *u, const xmlChar *n) {
    xmlEntityPtr r = xmlGetPredefinedEntity(n);
    if (!r && GCTX && GCTX->myDoc) r = xmlGetDocEntity(GCTX->myDoc, n);
    return r;
}
static void st(void *u, const xmlChar *n, const xmlChar **a){ fprintf(stderr,"ST<%s>\n",(char*)n); }
static void en(void *u, const xmlChar *n){ fprintf(stderr,"EN</%s>\n",(char*)n); }
static void cd(void *u, const xmlChar *s, int l){ fprintf(stderr,"CD[%.*s]\n",l,(char*)s); }

static const xmlSAXHandler handlers = {
    .internalSubset = NULL, .isStandalone = NULL, .hasInternalSubset = NULL,
    .hasExternalSubset = NULL, .resolveEntity = NULL, .getEntity = ge,
    .entityDecl = NULL, .notationDecl = NULL, .attributeDecl = NULL,
    .elementDecl = NULL, .unparsedEntityDecl = NULL, .setDocumentLocator = NULL,
    .startDocument = NULL, .endDocument = NULL, .startElement = st,
    .endElement = en, .reference = NULL, .characters = cd,
    .ignorableWhitespace = NULL, .processingInstruction = NULL, .comment = NULL,
    .warning = NULL, .error = NULL, .fatalError = NULL,
    .getParameterEntity = NULL, .cdataBlock = NULL, .externalSubset = NULL,
    .initialized = XML_SAX2_MAGIC, ._private = NULL, .startElementNs = NULL,
    .endElementNs = NULL, .serror = NULL,
};

int main(void) {
    const char *doc =
      "<?xml version=\"1.0\"?><!DOCTYPE root [<!ENTITY e \" \">]>"
      "<root><a>p&e;<b>z</b></a></root>";
    xmlParserCtxtPtr p = xmlCreatePushParserCtxt((xmlSAXHandlerPtr)&handlers,
        (void*)(const char*)"x", NULL, 0, NULL);
    GCTX = p;
    xmlCtxtUseOptions(p, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
    int total=(int)strlen(doc), pos=0, rc=0;
    while(pos<total){ int s=7; if(s>total-pos) s=total-pos; rc=xmlParseChunk(p,doc+pos,s,0); pos+=s; if(rc<0){fprintf(stderr,"chunk err %d@%d\n",rc,pos);break;} }
    rc=xmlParseChunk(p,doc+pos,0,1);
    fprintf(stderr,"final=%d\n",rc);
    if(p->myDoc) xmlFreeDoc(p->myDoc);
    xmlFreeParserCtxt(p);
    return 0;
}
