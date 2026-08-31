/* Probe: parse a doc and dump attribute ns/prefix/name bindings. */
#include <stdio.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

static void dump_attrs(xmlNodePtr cur) {
    for (xmlAttrPtr a = cur->properties; a != NULL; a = a->next) {
        printf("  attr name=%s ns=%s prefix=%s\n",
               (char *)(a->name ? a->name : (xmlChar *)"<null>"),
               (a->ns && a->ns->href) ? (char *)a->ns->href : (char *)"<null>",
               (a->ns && a->ns->prefix) ? (char *)a->ns->prefix : (char *)"<null>");
    }
}

int main(int argc, char **argv) {
    if (argc < 2) return 1;
    xmlDocPtr doc = xmlReadFile(argv[1], NULL, XML_PARSE_NONET);
    if (doc == NULL) { fprintf(stderr, "parse fail\n"); return 2; }
    for (xmlNodePtr cur = xmlDocGetRootElement(doc); cur != NULL; cur = cur->next) {
        printf("ELEMENT %s ns=%s prefix=%s\n",
               (char *)(cur->name ? cur->name : (xmlChar *)"<null>"),
               (cur->ns && cur->ns->href) ? (char *)cur->ns->href : (char *)"<null>",
               (cur->ns && cur->ns->prefix) ? (char *)cur->ns->prefix : (char *)"<null>");
        dump_attrs(cur);
        for (xmlNodePtr ch = cur->children; ch != NULL; ch = ch->next) {
            if (ch->type == XML_ELEMENT_NODE) {
                printf("  ELEMENT %s ns=%s prefix=%s\n",
                       (char *)(ch->name ? ch->name : (xmlChar *)"<null>"),
                       (ch->ns && ch->ns->href) ? (char *)ch->ns->href : (char *)"<null>",
                       (ch->ns && ch->ns->prefix) ? (char *)ch->ns->prefix : (char *)"<null>");
                dump_attrs(ch);
            }
        }
    }
    xmlFreeDoc(doc);
    return 0;
}
