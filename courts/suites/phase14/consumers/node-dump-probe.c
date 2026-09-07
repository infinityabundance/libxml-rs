/* node-dump-probe.c — print the raw DOM child sequence of the root element
 * (type + exact bytes) with NO serializer reformatting, so parser behavior
 * can be compared oracle-vs-candidate without --format contamination. */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

static void dump_bytes(const xmlChar *s, int len) {
    int i;
    for (i = 0; i < len; i++) {
        if (s[i] == '\n') printf("\\n");
        else if (s[i] == '\r') printf("\\r");
        else if (s[i] == '\t') printf("\\t");
        else if (s[i] >= 32 && s[i] < 127) printf("%c", s[i]);
        else printf("\\x%02x", s[i]);
    }
}

int main(int argc, char **argv) {
    int i;
    if (argc < 2) return 1;
    for (i = 1; i < argc; i++) {
        xmlDocPtr doc = xmlReadFile(argv[i], NULL, 0);
        if (doc == NULL) { printf("parse-error %s\n", argv[i]); continue; }
        xmlNodePtr root = xmlDocGetRootElement(doc);
        if (root == NULL) { printf("no-root %s\n", argv[i]); continue; }
        printf("== %s\n", argv[i]);
        /* attributes */
        for (xmlAttrPtr a = root->properties; a; a = a->next) {
            xmlNodePtr c = a->children;
            printf("  attr %s = [", a->name);
            while (c) { dump_bytes(c->content ? c->content : (xmlChar*)"", c->content ? xmlStrlen(c->content) : 0); c = c->next; }
            printf("]\n");
        }
        /* children */
        for (xmlNodePtr n = root->children; n; n = n->next) {
            const char *t = "?";
            switch (n->type) {
                case XML_TEXT_NODE: t = "text"; break;
                case XML_CDATA_SECTION_NODE: t = "cdata"; break;
                case XML_COMMENT_NODE: t = "comment"; break;
                case XML_PI_NODE: t = "pi"; break;
                case XML_ELEMENT_NODE: t = "element"; break;
                case XML_ENTITY_REF_NODE: t = "entityref"; break;
                default: break;
            }
            if (n->type == XML_TEXT_NODE || n->type == XML_CDATA_SECTION_NODE) {
                int len = n->content ? xmlStrlen(n->content) : 0;
                printf("  %s [", t);
                dump_bytes(n->content, len);
                printf("]\n");
            } else if (n->type == XML_COMMENT_NODE) {
                printf("  comment [");
                dump_bytes(n->content, n->content ? xmlStrlen(n->content) : 0);
                printf("]\n");
            } else if (n->type == XML_ELEMENT_NODE) {
                printf("  element <%s>\n", n->name);
            } else if (n->type == XML_ENTITY_REF_NODE) {
                printf("  entityref &%s;\n", n->name);
            } else {
                printf("  other type=%d\n", n->type);
            }
        }
        xmlFreeDoc(doc);
    }
    return 0;
}
