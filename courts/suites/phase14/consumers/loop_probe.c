#include <libxml/parser.h>
#include <libxml/xmlerror.h>
#include <stdio.h>
#include <string.h>

static void serror(void *ctx, const xmlError *err) {
    xmlParserCtxtPtr ctxt = (xmlParserCtxtPtr)ctx;
    printf("code=%d line=%d col=%d\n", err->code, err->line, err->int2);
    if (ctxt && ctxt->input) {
        printf("  cur input: line=%d col=%lu cur_off=%ld\n",
               ctxt->input->line, ctxt->input->col,
               (long)(ctxt->input->cur - ctxt->input->base));
    }
}

int main(void) {
    const char *docs[] = {
        "<r a=\"1\" b='2' c=3/>",
        "<!DOCTYPE r [<!ENTITY a '&a;'>]><r>&a;<bad><x></r>",
    };
    for (int i = 0; i < 2; i++) {
        xmlParserCtxtPtr ctxt = xmlNewParserCtxt();
        xmlCtxtSetErrorHandler(ctxt, serror, ctxt);
        xmlDocPtr d = xmlCtxtReadMemory(ctxt, docs[i], (int)strlen(docs[i]), "t.xml", NULL, 0);
        printf("doc%d=%p\n", i, (void *)d);
        xmlFreeDoc(d);
        xmlFreeParserCtxt(ctxt);
    }
    return 0;
}
