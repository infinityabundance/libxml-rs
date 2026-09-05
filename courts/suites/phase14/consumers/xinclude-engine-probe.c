#include <stdio.h>
#include <libxml/parser.h>
#include <libxml/xinclude.h>
#include <libxml/tree.h>

/* Replicates the Phase-14.23 xsltLoadDocument doXInclude step:
   read data.xml, xmlXIncludeProcessFlags(doc, 0), dump. */
int main(int argc, char **argv) {
    const char *path = argc > 1 ? argv[1] : "data.xml";
    xmlDocPtr doc = xmlReadFile(path, NULL, 0);
    if (doc == NULL) { printf("read failed\n"); return 1; }
    int rc = xmlXIncludeProcessFlags(doc, 0);
    printf("process rc=%d\n", rc);
    xmlDocDump(stdout, doc);
    xmlFreeDoc(doc);
    return 0;
}
