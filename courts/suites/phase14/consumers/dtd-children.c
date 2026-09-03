#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

int main(void) {
    const char *xml =
        "<!DOCTYPE r [\n"
        "    <!ELEMENT r (a)>\n"
        "    <!ELEMENT a (#PCDATA)>\n"
        "    <!ATTLIST a id ID #IMPLIED>\n"
        "    <!ENTITY foo \"x\">\n"
        "    <!NOTATION n SYSTEM \"n.dtd\">\n"
        "]>\n<r/>\n";
    xmlDocPtr d = xmlReadMemory(xml, (int) strlen(xml), NULL, NULL, 0);
    printf("doc=%p\n", (void *) d);
    if (!d) return 1;
    xmlNodePtr c = d->children;
    while (c) {
        printf("doc child type=%d name=%s\n", c->type, c->name ? (char*) c->name : "(null)");
        if (c->type == XML_DTD_NODE) {
            xmlNodePtr dc = c->children;
            while (dc) {
                printf("   dtd child type=%d name=%s\n", dc->type,
                       dc->name ? (char *) dc->name : "(null)");
                dc = dc->next;
            }
        }
        c = c->next;
    }
    printf("intSubset=%p children-first=%p\n", (void *) d->intSubset,
           (void *) d->children);
    xmlFreeDoc(d);
    return 0;
}
