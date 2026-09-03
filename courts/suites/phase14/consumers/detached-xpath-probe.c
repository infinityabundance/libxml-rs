#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>

/* What does an absolute path return when the context node is a DETACHED
 * deep copy (parent == NULL, doc set) living in the same doc? */
int main(void) {
    xmlDocPtr d = xmlReadMemory("<a><b/></a>", 11, NULL, NULL, 0);
    xmlNodePtr orig = xmlDocGetRootElement(d);
    xmlNodePtr copy = xmlDocCopyNode(orig, d, 1);
    printf("copy parent=%p doc==d:%d children=%p\n", (void *) copy->parent,
           copy->doc == d, (void *) copy->children);

    xmlXPathContextPtr ctx = xmlXPathNewContext(d);
    ctx->node = copy;
    xmlXPathObjectPtr res = xmlXPathEvalExpression(BAD_CAST "/a", ctx);
    printf("res type=%d nodeNr=%d\n", res ? res->type : -1,
           (res && res->nodesetval) ? res->nodesetval->nodeNr : -1);
    if (res && res->nodesetval && res->nodesetval->nodeNr > 0) {
        xmlNodePtr hit = res->nodesetval->nodeTab[0];
        printf("hit == orig:%d hit == copy:%d hit->parent=%p name=%s\n",
               hit == orig, hit == copy, (void *) hit->parent,
               hit->name ? (const char *) hit->name : "(null)");
    }
    if (res) xmlXPathFreeObject(res);
    xmlXPathFreeContext(ctx);
    /* free the detached copy manually (not linked into the doc) */
    if (copy) xmlFreeNode(copy);
    xmlFreeDoc(d);
    return 0;
}
