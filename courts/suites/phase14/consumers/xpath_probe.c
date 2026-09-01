#include <libxml/xpath.h>
#include <libxml/xmlerror.h>
#include <libxml/tree.h>
#include <stdio.h>
#include <string.h>

static int structured_calls = 0;
static int generic_calls = 0;

static void serror(void *ctx, const xmlError *err) {
    structured_calls++;
    printf("  [structured] code=%d level=%d msg=%s\n",
           err->code, err->level, err->message ? err->message : "(null)");
}

static void gerror(void *ctx, const char *msg, ...) {
    generic_calls++;
    printf("  [generic] %s", msg);
}

int main(void) {
    xmlDocPtr doc = xmlReadMemory("<r><a>1</a></r>", 13, "t.xml", NULL, 0);
    xmlXPathContextPtr ctxt = xmlXPathNewContext(doc);
    ctxt->error = serror;        /* like lxml's structured slot */
    ctxt->userData = ctxt;
    xmlSetGenericErrorFunc(NULL, gerror);

    xmlXPathObjectPtr o1 = xmlXPathEvalExpression(BAD_CAST "//a[", ctxt);
    printf("bad-expr result=%p\n", (void *)o1);
    printf("structured=%d generic=%d\n", structured_calls, generic_calls);

    xmlXPathObjectPtr o2 = xmlXPathEvalExpression(BAD_CAST "//n:c", ctxt);
    printf("bad-prefix result=%p\n", (void *)o2);
    printf("structured=%d generic=%d\n", structured_calls, generic_calls);

    xmlXPathFreeObject(o1);
    xmlXPathFreeObject(o2);
    xmlXPathFreeContext(ctxt);
    xmlFreeDoc(doc);
    return 0;
}
