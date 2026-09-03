#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/xmlsave.h>

static int wcb(void *ctx, const char *buf, int len) {
    (void) ctx;
    fwrite(buf, 1, (size_t) len, stdout);
    return len;
}

int main(void) {
    const char *xml =
        "<?xml version='1.0' encoding='utf-8'?>\n"
        "<!DOCTYPE set [\n"
        "    <!ENTITY foo '<foo>bar</foo>'>\n"
        "    <!ENTITY xxe SYSTEM \"file:///etc/passwd\">\n"
        "]>\n"
        "<set>&foo;&xxe;</set>\n";
    xmlDocPtr d = xmlReadMemory(xml, (int) strlen(xml), NULL, NULL,
                                XML_PARSE_NOENT | XML_PARSE_NO_XXE);
    printf("doc=%p\n", (void *) d);
    xmlSaveCtxtPtr ctxt = xmlSaveToIO(wcb, NULL, NULL, NULL, XML_SAVE_AS_XML);
    printf("=== xmlSaveDoc ===\n");
    xmlSaveDoc(ctxt, d);
    xmlSaveFlush(ctxt);
    printf("\n=== xmlSaveTree(dtd) after SaveDoc ===\n");
    xmlNodePtr dtd = d->intSubset ? (xmlNodePtr) d->intSubset : NULL;
    printf("intSubset=%p children-listed below\n", (void *) dtd);
    if (dtd) {
        xmlSaveTree(ctxt, dtd);
        xmlSaveFlush(ctxt);
    }
    printf("\n=== xmlSaveDoc again (fresh ctxt) ===\n");
    xmlSaveClose(ctxt);
    ctxt = xmlSaveToIO(wcb, NULL, NULL, NULL, XML_SAVE_AS_XML);
    xmlSaveDoc(ctxt, d);
    xmlSaveClose(ctxt);
    printf("\n");
    xmlFreeDoc(d);
    return 0;
}
