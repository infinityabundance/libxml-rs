// php_html_ctxt_probe.c — exercise htmlCreateMemoryParserCtxt + htmlParseDocument + myDoc.
#include <stdio.h>
#include <string.h>
#include <libxml/HTMLparser.h>
#include <libxml/tree.h>
int main(void) {
    const char *src = "<html><body><p>Test<br></p></body></html>";
    htmlParserCtxtPtr ctxt = htmlCreateMemoryParserCtxt(src, (int)strlen(src));
    if (!ctxt) { printf("ctxt NULL\n"); return 1; }
    printf("ctxt=%p sax=%p myDoc=%p vctxt.error=%p\n",
           (void*)ctxt, (void*)ctxt->sax, (void*)ctxt->myDoc,
           (void*)ctxt->vctxt.error);
    int rc = htmlParseDocument(ctxt);
    printf("parse rc=%d myDoc=%p\n", rc, (void*)ctxt->myDoc);
    xmlDocPtr doc = ctxt->myDoc;
    if (doc) {
        xmlNodePtr root = doc->children;
        printf("doc children=%p root=%s\n", (void*)root, root && root->name ? (char*)root->name : "(null)");
    }
    htmlFreeParserCtxt(ctxt);
    return 0;
}
