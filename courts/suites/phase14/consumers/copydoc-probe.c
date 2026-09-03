#include <stdio.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

/* After xmlCopyDoc(d, 1): check that every node in the copy belongs to the
 * NEW document and that the new root's parent is the new doc node. */
static void walk(xmlNodePtr n, xmlDocPtr expect, int depth, int *bad) {
    for (; n; n = n->next) {
        if (n->doc != expect) {
            printf("  BAD doc: depth=%d name=%s doc-mismatch\n", depth,
                   n->name ? (const char *) n->name : "(anon)");
            (*bad)++;
        }
        if (n->type == XML_ELEMENT_NODE) {
            if (n->children) walk(n->children, expect, depth + 1, bad);
            if (n->properties) {
                xmlAttrPtr a = n->properties;
                for (; a; a = a->next) {
                    if (a->doc != expect) {
                        printf("  BAD attr doc: %s\n", a->name ? (const char *) a->name : "?");
                        (*bad)++;
                    }
                }
            }
        }
    }
}

int main(void) {
    const char *src = "<a x='1'><b/><c>t</c></a>";
    xmlDocPtr d = xmlReadMemory(src, strlen(src), NULL, NULL, 0);
    xmlDocPtr c = xmlCopyDoc(d, 1);
    xmlNodePtr oroot = xmlDocGetRootElement(d);
    xmlNodePtr croot = xmlDocGetRootElement(c);
    printf("croot!=oroot:%d croot->doc==c:%d croot->parent==(xmlNodePtr)c:%d\n",
           croot != oroot, croot->doc == c,
           croot->parent == (xmlNodePtr) c);
    int bad = 0;
    walk((xmlNodePtr) c, c, 0, &bad);
    printf("doc(c)==d:%d bad=%d\n", c->doc == d, bad);
    xmlFreeDoc(c);
    xmlFreeDoc(d);
    return 0;
}
