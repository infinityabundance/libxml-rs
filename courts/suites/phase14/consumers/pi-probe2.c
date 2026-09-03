#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

int main(void) {
    const char *xml =
        "<?xml version=\"1.0\"?>\n<container>\n  <?foo pi contents ?>\n</container>\n";
    xmlDocPtr d = xmlReadMemory(xml, (int) strlen(xml), NULL, NULL, 0);
    xmlNodePtr c = d ? d->children : NULL;
    while (c) {
        if (c->type == XML_ELEMENT_NODE) {
            xmlNodePtr ch = c->children;
            while (ch) {
                if (ch->type == XML_PI_NODE) {
                    printf("PI name=%s content=[%s] len=%d (raw content ptr)\n",
                           ch->name ? (char *) ch->name : "(null)",
                           ch->content ? (char *) ch->content : "(null)",
                           ch->content ? (int) strlen((char *) ch->content) : -1);
                }
                ch = ch->next;
            }
        }
        c = c->next;
    }
    xmlFreeDoc(d);
    return 0;
}
