/* C14N API probe: whole-doc + subset + comments + execute, vs oracle. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>
#include <libxml/c14n.h>

static int write_cb(void *ctx, const char *data, int len) {
    fwrite(data, 1, len, stdout);
    return len;
}

int main(int argc, char **argv) {
    if (argc < 2) return 1;
    int mode = atoi(argv[2] ? argv[2] : "0");   /* 0=1.0 1=exc 2=1.1 */
    int with_comments = atoi(argv[3] ? argv[3] : "1");
    const char *subset = (argc > 4) ? argv[4] : NULL; /* e.g. "a" selects all <a> elements */
    xmlDocPtr doc = xmlReadFile(argv[1], NULL, XML_PARSE_NONET);
    if (doc == NULL) { fprintf(stderr, "parse fail\n"); return 2; }

    /* Build an optional subset: all elements named `subset` (document order).
     * `*` = all elements, `#text` = all text nodes. */
    xmlNodeSetPtr nodes = NULL;
    if (subset != NULL) {
        nodes = xmlXPathNodeSetCreate(NULL);
        for (xmlNodePtr cur = xmlDocGetRootElement(doc); cur != NULL; cur = cur->next) {
            xmlNodePtr stack[1024]; int sp = 0;
            stack[sp++] = cur;
            while (sp > 0) {
                xmlNodePtr n = stack[--sp];
                int want = 0;
                if (strcmp(subset, "*") == 0 && n->type == XML_ELEMENT_NODE) want = 1;
                else if (strcmp(subset, "#text") == 0 &&
                         (n->type == XML_TEXT_NODE || n->type == XML_CDATA_SECTION_NODE)) want = 1;
                else if (n->type == XML_ELEMENT_NODE && xmlStrEqual(n->name, (const xmlChar *)subset)) want = 1;
                if (want) {
                    if (nodes->nodeNr >= nodes->nodeMax) {
                        int ncap = nodes->nodeMax ? nodes->nodeMax * 2 : 8;
                        nodes->nodeTab = (xmlNodePtr *)realloc(nodes->nodeTab, ncap * sizeof(xmlNodePtr));
                        nodes->nodeMax = ncap;
                    }
                    nodes->nodeTab[nodes->nodeNr++] = n;
                }
                for (xmlNodePtr ch = n->children; ch != NULL; ch = ch->next)
                    stack[sp++] = ch;
            }
        }
    }

    xmlChar *result = NULL;
    int len = xmlC14NDocDumpMemory(doc, nodes, mode,
                                   NULL, with_comments, &result);
    if (nodes != NULL) xmlXPathFreeNodeSet(nodes);
    if (len < 0) {
        printf("FAILED\n");
    } else if (result != NULL) {
        fwrite(result, 1, len, stdout);
        printf("\n");
        xmlFree(result);
    }
    xmlFreeDoc(doc);
    return 0;
}
