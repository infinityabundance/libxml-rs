#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

static void dump(xmlDocPtr d, const char *label) {
    xmlChar *mem = NULL;
    int size = 0;
    xmlDocDumpMemory(d, &mem, &size);
    printf("== %s ==\n%.*s\n", label, size, mem ? (char *) mem : "");
    xmlFree(mem);
}

int main(void) {
    const char *s1 = "<source xmlns:foo=\"some:ns\" foo:bar=\"1\"/>";
    const char *s2 = "<container xmlns:foo=\"some:other\"/>";
    xmlDocPtr src = xmlReadMemory(s1, strlen(s1), NULL, NULL, 0);
    xmlDocPtr dst = xmlReadMemory(s2, strlen(s2), NULL, NULL, 0);
    xmlNodePtr root = xmlDocGetRootElement(src);
    xmlAttrPtr sattr = NULL;
    for (xmlAttrPtr a = root->properties; a; a = a->next) {
        if (a->ns) { sattr = a; break; }
    }
    printf("src attr=%p ns->prefix=%s ns->href=%s\n", (void *) sattr,
           sattr->ns->prefix ? (const char *) sattr->ns->prefix : "(null)",
           sattr->ns->href ? (const char *) sattr->ns->href : "(null)");
    xmlAttrPtr imp = xmlDocCopyNode((xmlNodePtr) sattr, dst, 0);
    printf("imported attr=%p ns=%p ns->prefix=%s ns->href=%s\n", (void *) imp,
           imp ? (void *) imp->ns : NULL,
           imp && imp->ns && imp->ns->prefix ? (const char *) imp->ns->prefix : "(null)",
           imp && imp->ns && imp->ns->href ? (const char *) imp->ns->href : "(null)");
    xmlAddChild(xmlDocGetRootElement(dst), (xmlNodePtr) imp);
    dump(dst, "dst after importing attr (prefix conflict)");

    xmlFreeDoc(src);
    xmlFreeDoc(dst);
    return 0;
}
