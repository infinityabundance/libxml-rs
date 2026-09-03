#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

/* Replicate php's createAttributeNS + setAttributeNode EXACTLY (legacy):
 * step1: nsptr1 = xmlNewNs(root, ns1, "foo")  [createAttributeNS's
 *         dom_get_ns_unchecked adds the decl to root at CREATE time]
 * step2: a1 = xmlNewDocProp(doc, "hello", NULL); a1->ns = nsptr1
 * step3: setAttributeNode(a1): xmlHasProp(root,"hello"); xmlAddChild(root,a1)
 * step4: nsptr2 = xmlNewNs(root, ns2, "foo")  [should be NULL -> resolve]
 * step5: a2 = xmlNewDocProp(doc,"hello",NULL); a2->ns = resolved-ns2
 * step6: setAttributeNode(a2): xmlHasProp(root,"hello"); xmlAddChild(root,a2)
 * print the properties chain after each step. */

static void dump_attrs(xmlNodePtr n, const char *tag) {
    int i = 0;
    for (xmlAttrPtr a = n->properties; a; a = a->next) {
        printf("  %s attr[%d] name=%s ns=%s prefix=%s\n", tag, i++,
               (const char *) a->name,
               a->ns && a->ns->href ? (const char *) a->ns->href : "(null)",
               a->ns && a->ns->prefix ? (const char *) a->ns->prefix : "(null)");
    }
    printf("  %s total=%d hasProp(hello)=%s\n", tag, i,
           xmlHasProp(n, BAD_CAST "hello") ? "FOUND" : "none");
}

int main(void) {
    xmlDocPtr d = xmlNewDoc(BAD_CAST "1.0");
    xmlNodePtr root = xmlNewDocNode(d, NULL, BAD_CAST "container", NULL);
    xmlDocSetRootElement(d, root);

    xmlNsPtr ns1 = xmlNewNs(root, BAD_CAST "http://php.net/ns1", BAD_CAST "foo");
    xmlAttrPtr a1 = xmlNewDocProp(d, BAD_CAST "hello", NULL);
    a1->ns = ns1;
    xmlAddChild(root, (xmlNodePtr) a1);
    dump_attrs(root, "after-op1");

    xmlNsPtr ns2 = xmlNewNs(root, BAD_CAST "http://php.net/ns2", BAD_CAST "foo");
    printf("xmlNewNs(root, ns2, foo) = %s\n", ns2 ? "CREATED" : "NULL(conflict)");
    if (ns2 == NULL) {
        /* dom_get_ns_resolve_prefix_conflict: default, default1... */
        char prefix[50];
        int counter = 1;
        snprintf(prefix, sizeof(prefix), "default");
        while (xmlSearchNs(d, root, (const xmlChar *) prefix) != NULL) {
            snprintf(prefix, sizeof(prefix), "default%d", counter++);
        }
        ns2 = xmlNewNs(root, BAD_CAST "http://php.net/ns2", BAD_CAST prefix);
    }
    xmlAttrPtr a2 = xmlNewDocProp(d, BAD_CAST "hello", NULL);
    a2->ns = ns2;
    xmlAddChild(root, (xmlNodePtr) a2);
    dump_attrs(root, "after-op2");

    xmlFreeDoc(d);
    return 0;
}
