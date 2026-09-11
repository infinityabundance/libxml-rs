#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxslt/xslt.h>
#include <libxslt/transform.h>
#include <libxslt/xsltInternals.h>
#include <libexslt/exslt.h>
int main(int argc, char **argv) {
    xsltRegisterAllExtras();
    exsltRegisterAll();
    xsltStylesheetPtr st = xsltParseStylesheetFile((const xmlChar *)argv[1]);
    if (!st) { printf("stylesheet parse FAILED\n"); return 1; }
    xmlDocPtr doc;
    if (argc > 2 && argv[2][0]) doc = xmlReadFile(argv[2], NULL, 0);
    else doc = xmlReadMemory("<r/>", 4, "s", NULL, 0);
    if (!doc) { printf("doc parse FAILED\n"); return 1; }
    xmlDocPtr res = xsltApplyStylesheet(st, doc, NULL);
    if (!res) { printf("transform FAILED\n"); return 1; }
    xmlChar *out = NULL; int len = 0;
    xmlDocDumpMemory(res, &out, &len);
    printf("%.*s\n", len, out ? (char*)out : "");
    if (out) xmlFree(out);
    return 0;
}
