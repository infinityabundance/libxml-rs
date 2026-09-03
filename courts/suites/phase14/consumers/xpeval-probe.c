#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/xpath.h>
#include <libxml/xmlerror.h>

static void structured_handler(void *userData, const xmlError *error) {
    (void) userData;
    printf("  [structured] code=%d domain=%d level=%d msg=[%s]\n", error->code,
           error->domain, error->level, error->message ? error->message : "(null)");
}

int main(void) {
    xmlDocPtr d = xmlReadMemory("<sxe><a/></sxe>", 18, NULL, NULL, 0);
    xmlXPathContextPtr ctx = xmlXPathNewContext(d);
    ctx->userData = ctx;
    ctx->error = structured_handler;
    xmlXPathObjectPtr res = xmlXPathEvalExpression(BAD_CAST "**", ctx);
    printf("res=%p\n", (void *) res);
    if (ctx->lastError.message)
        printf("lastError.msg=[%s] code=%d\n", ctx->lastError.message,
               ctx->lastError.code);
    if (res) xmlXPathFreeObject(res);
    xmlXPathFreeContext(ctx);
    xmlFreeDoc(d);
    return 0;
}
