/*
 * test-libxml.c — external C consumer probe for the 11.1-S build courts.
 *
 * Court family: BUILD-PKGCONFIG / BUILD-CONFIG-SCRIPT (11.1-S).
 *
 * A consumer program written with ONLY standard C knowledge: it uses the
 * libxml2 API through `<libxml/parser.h>` and `<libxml/tree.h>` and must be
 * compilable with the standard toolchain commands:
 *
 *   cc $(xml2-config --cflags) test-libxml.c $(xml2-config --libs)
 *   cc $(pkg-config --cflags libxml-2.0) test-libxml.c $(pkg-config --libs libxml-2.0)
 *
 * It parses an in-memory document, dumps it and reports the runtime version.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

int main(void) {
    xmlInitParser();
    const char *doc = "<root a='1'><child>hello</child></root>";
    xmlDocPtr d = xmlReadMemory(doc, (int)strlen(doc), "test.xml", NULL, 0);
    if (d == NULL) {
        fprintf(stderr, "xmlReadMemory failed\n");
        return 1;
    }
    xmlChar *dump = NULL;
    int len = 0;
    xmlDocDumpFormatMemory(d, &dump, &len, 1);
    if (dump == NULL) {
        fprintf(stderr, "xmlDocDumpFormatMemory failed\n");
        xmlFreeDoc(d);
        return 1;
    }
    printf("version=%s\n", xmlParserVersion);
    printf("dump=%s", (char *)dump);
    xmlFree(dump);
    xmlFreeDoc(d);
    xmlCleanupParser();
    return 0;
}
