#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/HTMLparser.h>

static void dump_el(const char *label, xmlNodePtr el) {
    printf("== %s ==\n", label);
    for (xmlAttrPtr a = el->properties; a; a = a->next) {
        printf("  attr name=%s ns=%s id=%p atype=%d val=",
               (char *) a->name,
               a->ns && a->ns->prefix ? (char *) a->ns->prefix : "(null)",
               (void *) a->id, a->atype);
        if (a->children && a->children->content) printf("%s", (char *) a->children->content);
        printf("\n");
    }
    xmlAttrPtr found = xmlGetID(el->doc, BAD_CAST "x");
    printf("  xmlGetID(x) -> %p %s\n", (void *) found,
           found && found->parent ? (char *) found->parent->name : "");
    found = xmlGetID(el->doc, BAD_CAST "test");
    printf("  xmlGetID(test) -> %p\n", (void *) found);
}

int main(void) {
    /* xml:id in XML */
    const char *xmlsrc = "<root><test1 xml:id=\"x\"/><test2 xml:id=\"x\"/></root>";
    xmlDocPtr d = xmlReadMemory(xmlsrc, strlen(xmlsrc), NULL, NULL, 0);
    xmlNodePtr root = xmlDocGetRootElement(d);
    dump_el("xml:id", root->children); /* test1 */
    xmlFreeDoc(d);

    /* plain id in HTML */
    const char *htmlsrc = "<p id=\"test\">foo</p>";
    xmlDocPtr h = htmlReadMemory(htmlsrc, strlen(htmlsrc), NULL, NULL, HTML_PARSE_NOERROR | HTML_PARSE_NOIMPLIED);
    xmlNodePtr p = xmlDocGetRootElement(h);
    dump_el("html id", p);
    xmlFreeDoc(h);
    return 0;
}
