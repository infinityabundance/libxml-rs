#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/HTMLparser.h>
#include <libxml/HTMLtree.h>

int main(void) {
    const char *src = "<html><body><p>a&nbsp;b&#160;c&eacute;d</p></body></html>";
    htmlDocPtr d = htmlReadMemory(src, strlen(src), NULL, NULL, 0);
    /* inspect the text node */
    xmlNodePtr p = d->children;
    while (p && p->type != XML_ELEMENT_NODE) p = p->next;
    xmlNodePtr body = p ? p->children : NULL;
    while (body && body->type != XML_ELEMENT_NODE) body = body->next;
    xmlNodePtr par = body ? body->children : NULL;
    while (par && par->type != XML_ELEMENT_NODE) par = par->next;
    xmlNodePtr t = par ? par->children : NULL;
    if (t) printf("text type=%d name=%s content=%s\n", t->type,
                  t->name ? (const char *) t->name : "(null)",
                  t->content ? (const char *) t->content : "(null)");
    xmlChar *mem = NULL;
    int size = 0;
    htmlDocDumpMemoryFormat(d, &mem, &size, 0);
    printf("dump: %s\n", mem ? (const char *) mem : "(null)");
    if (mem) xmlFree(mem);
    xmlFreeDoc(d);
    return 0;
}
