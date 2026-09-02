/* entprobe.c — replay PHP ext/xml compat's get_entity resolution chain.
 * Determines whether a DTD-internal-subset general entity is registered in
 * myDoc->entities after a compat-style push SAX parse, and whether
 * xmlGetDocEntity resolves it.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

static xmlParserCtxtPtr GCTX = 0;
static void st2(void *u, const xmlChar *n, const xmlChar **a){ fprintf(stderr,"ST<%s>\n",(char*)n); }
static void en2(void *u, const xmlChar *n){ fprintf(stderr,"EN</%s>\n",(char*)n); }
static void cd_cb(void *u, const xmlChar *s, int len){ fprintf(stderr,"CD[%.*s]\n",len,s); }
static xmlEntityPtr ge(void *u, const xmlChar *name){
    xmlEntityPtr r = xmlGetPredefinedEntity(name);
    if(!r && GCTX && GCTX->myDoc) r = xmlGetDocEntity(GCTX->myDoc, name);
    return r;
}
static const xmlSAXHandler handlers = {
    .internalSubset = NULL,
    .isStandalone = NULL,
    .hasInternalSubset = NULL,
    .hasExternalSubset = NULL,
    .resolveEntity = NULL,
    .getEntity = ge,   /* getEntity present (mirrors PHP ext/xml compat) */
    .entityDecl = NULL,
    .notationDecl = NULL,
    .attributeDecl = NULL,
    .elementDecl = NULL,
    .unparsedEntityDecl = NULL,
    .setDocumentLocator = NULL,
    .startDocument = NULL,
    .endDocument = NULL,
    .startElement = st2,
    .endElement = en2,
    .reference = NULL,
    .characters = cd_cb,
    .ignorableWhitespace = NULL,
    .processingInstruction = NULL,
    .comment = NULL,
    .warning = NULL,
    .error = NULL,
    .fatalError = NULL,
    .getParameterEntity = NULL,
    .cdataBlock = NULL,
    .externalSubset = NULL,
    .initialized = XML_SAX2_MAGIC,
    ._private = NULL,
    .startElementNs = NULL,
    .endElementNs = NULL,
    .serror = NULL,
};

int main(void){
    const char *docs[] = {
        "<?xml version=\"1.0\"?><!DOCTYPE root [<!ENTITY e \"ENT\">]>"
          "<root a=\"x&e;y\">x&e;y</root>",
        "<?xml version=\"1.0\"?><!DOCTYPE root [<!ENTITY e \"ENT\">]>"
          "<root>ab&e;cd</root>",
        "<?xml version=\"1.0\"?><!DOCTYPE root [<!ENTITY e \"E<b>&amp;</b>F\">]>"
          "<root>p&e;q</root>",
        "<?xml version=\"1.0\"?><!DOCTYPE root [<!ENTITY e \"ENT\">]>"
          "<root>&e;</root>",
    };
    int d;
    for(d=0; d<(int)(sizeof(docs)/sizeof(docs[0])); d++){
        fprintf(stderr,"===== doc %d =====\n", d);
        xmlParserCtxtPtr p = xmlCreatePushParserCtxt((xmlSAXHandlerPtr)&handlers, (void*)((const char*)"NONCTXT"), NULL, 0, NULL);
        if(!p){ fprintf(stderr,"no ctxt\n"); return 1; }
        GCTX = p;
        xmlCtxtUseOptions(p, XML_PARSE_OLDSAX | XML_PARSE_NOENT);
        const char *m = docs[d];
        int total=(int)strlen(m), pos=0, rc=0;
        while(pos<total){ int step=7; if(step>total-pos) step=total-pos; rc=xmlParseChunk(p,m+pos,step,0); pos+=step; if(rc<0){fprintf(stderr,"chunk rc=%d at %d\n",rc,pos); break;} }
        if(pos>=total) rc=xmlParseChunk(p,m+pos,0,1);
        fprintf(stderr,"final parse rc=%d\n", rc);
        if(p->myDoc){ xmlFreeDoc(p->myDoc); p->myDoc=NULL; } else fprintf(stderr,"no myDoc retained\n");
        xmlFreeParserCtxt(p);
    }
    return 0;
}
