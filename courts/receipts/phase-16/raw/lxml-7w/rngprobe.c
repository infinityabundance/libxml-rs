#include <stdio.h>
#include <stdarg.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/relaxng.h>

static void err(void *ctx, const char *msg, ...) {
    va_list ap;
    va_start(ap, msg);
    (void)ctx;
    vfprintf(stderr, msg, ap);
    va_end(ap);
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: %s schema.rng doc.xml\n", argv[0]); return 2; }
    xmlRelaxNGParserCtxtPtr pctxt = xmlRelaxNGNewParserCtxt(argv[1]);
    xmlRelaxNGSetParserErrors(pctxt, err, err, NULL);
    xmlRelaxNGPtr schema = xmlRelaxNGParse(pctxt);
    if (!schema) { printf("RNG_PARSE_FAILED\n"); return 1; }
    printf("RNG_PARSE_OK\n");
    xmlRelaxNGValidCtxtPtr vctxt = xmlRelaxNGNewValidCtxt(schema);
    xmlRelaxNGSetValidErrors(vctxt, err, err, NULL);
    xmlDocPtr doc = xmlReadFile(argv[2], NULL, 0);
    if (!doc) { printf("DOC_PARSE_FAILED\n"); return 1; }
    int rc = xmlRelaxNGValidateDoc(vctxt, doc);
    printf("VALIDATE_RC=%d\n", rc);
    return 0;
}
