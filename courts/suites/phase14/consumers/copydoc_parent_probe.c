/* Differential court probe: xmlCopyDoc root parenting.
 *
 * Mirrors the Phase 14.3 Bug-3 PHP path: a parsed document is deep-copied via
 * xmlCopyDoc, then the copy is navigated (firstChild chains) and both docs are
 * freed. Upstream xmlCopyDoc keeps the copied document node as the parent of
 * every top-level child, so consumers that treat a NULL node parent as
 * "ownerless" (PHP php_libxml_node_free_resource) must not see the copied
 * root as ownerless - otherwise the subtree is freed early and xmlFreeDoc
 * double-frees it.
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
        const char *x = "<p><b>hello</b><b><i>world</i></b></p>";
        xmlDocPtr d = xmlReadMemory(x, (int) strlen(x), NULL, NULL, XML_PARSE_NONET);
        if (!d) { fprintf(stderr, "parsefail\n"); return 3; }
        xmlDocPtr c = xmlCopyDoc(d, 1);
        if (!c) { xmlFreeDoc(d); fprintf(stderr, "copyfail\n"); return 4; }
        /* navigate like PHP proxies do */
        xmlNodePtr root = xmlDocGetRootElement(c);
        if (root && root->children && root->children->next) {
            xmlNodePtr b1 = root->children;
            xmlNodePtr b2 = b1->next;
            (void) b2;
        }
        xmlFreeDoc(c);
        xmlFreeDoc(d);
    }
    printf("OK copydoc-parent %d\n", n);
    return 0;
}
