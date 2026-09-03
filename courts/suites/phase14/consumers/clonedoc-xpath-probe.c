#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>

/* The php SimpleXML root-element clone sequence (bug63575):
 *   docp = xmlCopyDoc(sxe->document->ptr, 1);
 *   nodep = xmlDocGetRootElement(docp);
 *   ctxt = xmlXPathNewContext(docp); ctxt->node = nodep;
 *   res = xmlXPathEval("/a", ctxt);
 * Which /a comes back — the copy's or the original's? */
int main(void) {
    const char *src = "<a><b></b></a>";
    xmlDocPtr d = xmlReadMemory(src, strlen(src), NULL, NULL, 0);
    xmlDocPtr c = xmlCopyDoc(d, 1);
    xmlNodePtr orig = xmlDocGetRootElement(d);
    xmlNodePtr croot = xmlDocGetRootElement(c);
    xmlXPathContextPtr ctx = xmlXPathNewContext(c);
    ctx->node = croot;
    xmlXPathObjectPtr res = xmlXPathEvalExpression(BAD_CAST "/a", ctx);
    printf("res=%p nodeNr=%d\n", (void *) res,
           (res && res->nodesetval) ? res->nodesetval->nodeNr : -1);
    if (res && res->nodesetval && res->nodesetval->nodeNr > 0) {
        xmlNodePtr hit = res->nodesetval->nodeTab[0];
        printf("hit==orig:%d hit==croot:%d hit->doc==c:%d hit->doc==d:%d\n",
               hit == orig, hit == croot, hit->doc == c, hit->doc == d);
    }
    if (res) xmlXPathFreeObject(res);
    xmlXPathFreeContext(ctx);
    xmlFreeDoc(c);
    xmlFreeDoc(d);
    return 0;
}
