#include <stdio.h>
#include <string.h>
#include <libxml/tree.h>
#include <libxml/xmlsave.h>
#include <libxml/xmlmemory.h>

/* Mimic php: adopt a docless text node into a doc, then save just that node. */
static int write_cb(void *ctx, const char *buf, int len) {
    fwrite(buf, 1, (size_t) len, (FILE *) ctx);
    return len;
}

int main(void) {
    xmlDocPtr doc = xmlNewDoc(BAD_CAST "1.0");
    xmlNodePtr text = xmlNewText(BAD_CAST "x");
    printf("text node=%p doc=%p\n", (void*)text, (void*)text->doc);
    int ret = xmlDOMWrapAdoptNode(NULL, NULL, text, doc, NULL, 0);
    printf("adopt ret=%d text->doc=%p doc=%p\n", ret, (void*)text->doc, (void*)doc);
    /* serialize the node alone like php dom_xml_serialize -> xmlSaveTree */
    xmlSaveCtxtPtr ctxt = xmlSaveToIO(write_cb, NULL, stdout, NULL, XML_SAVE_AS_XML);
    printf("ctxt=%p\n", (void*)ctxt);
    int sret = xmlSaveTree(ctxt, text);
    printf("\nsaveTree ret=%d\n", sret);
    xmlSaveClose(ctxt);
    xmlFreeDoc(doc);
    printf("freed doc ok\n");
    return 0;
}
