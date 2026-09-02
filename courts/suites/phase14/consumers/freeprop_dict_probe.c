/* Differential court probe: xmlFreeProp on a dict-interned attribute name.
 *
 * Mirrors the Phase 14.3 Bug-2 PHP path: SimpleXML `unset($sxe['id'])` on a
 * PARSED document runs xmlUnlinkNode(attr) then xmlFreeProp(attr). The parser
 * interns the attribute name in the document dictionary, so xmlFreeProp must
 * NOT free the interned name (DICT_FREE); xmlDictFree at doc teardown owns it.
 * A candidate that frees the name here double-frees at xmlDictFree.
 *
 * Oracle and candidate must both print the same lines and exit 0.
 */
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int n = 20000;
int main(int argc, char **argv) {
    if (argc > 1) n = atoi(argv[1]);
    for (int i = 0; i < n; i++) {
        const char *x = "<sxe id=\"elem1\"><elem1 attr1=\"first\" /></sxe>";
        xmlDocPtr d = xmlReadMemory(x, (int) strlen(x), NULL, NULL, XML_PARSE_NONET);
        if (!d) { fprintf(stderr, "parsefail\n"); return 3; }
        xmlNodePtr r = xmlDocGetRootElement(d);
        xmlAttrPtr a = xmlHasProp(r, BAD_CAST "id");
        if (a) {
            /* SimpleXML sxe_unlink_node(attr) on unset */
            xmlUnlinkNode((xmlNodePtr) a);
            if (!((xmlNodePtr) a)->_private) {
                xmlFreeProp(a);
            }
        }
        xmlFreeDoc(d);
    }
    printf("OK freeprop-dict %d\n", n);
    return 0;
}
