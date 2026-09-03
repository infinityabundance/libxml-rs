#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

/* gh19612 / modern serialize family: declared general entity referenced in an
 * ATTRIBUTE value. How is the attr represented, and how does each serializer
 * round-trip it? */

static const char DOC[] =
    "<?xml version=\"1.0\"?>\n"
    "<!DOCTYPE root [ <!ENTITY foo \"FOO\"> ]>\n"
    "<root a=\"&foo;bar&foo;\" b=\"&amp;\">t</root>\n";

static void walk_attr(xmlAttrPtr a) {
    printf("  attr %s children:", (const char *) a->name);
    for (xmlNodePtr c = a->children; c != NULL; c = c->next) {
        if (c->type == XML_ENTITY_REF_NODE)
            printf(" [ENTREF %s]", (const char *) c->name);
        else if (c->type == XML_TEXT_NODE)
            printf(" [TXT %s]", (const char *) (c->content ? c->content : (xmlChar *) "(null)"));
        else
            printf(" [type%d]", c->type);
    }
    printf("\n");
}

int main(void) {
    xmlDocPtr d = xmlReadMemory(DOC, (int) strlen(DOC), "t.xml", NULL, 0);
    printf("doc=%s\n", d ? "parsed" : "NULL");
    if (!d) return 1;
    xmlNodePtr root = xmlDocGetRootElement(d);
    for (xmlAttrPtr a = root->properties; a != NULL; a = a->next)
        walk_attr(a);
    xmlChar *s = NULL;
    int len = 0;
    xmlDocDumpFormatMemory(d, &s, &len, 0);
    printf("--- xml dump rc len=%d ---\n%s\n--- end ---\n", len, s ? (const char *) s : "(null)");
    if (s) xmlFree(s);
    xmlFreeDoc(d);
    return 0;
}
