#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/HTMLparser.h>

static void dump_node(xmlNodePtr n, int depth) {
    for (; n; n = n->next) {
        if (n->type == XML_ELEMENT_NODE) {
            printf("%*s<%s>\n", depth * 2, "", n->name ? (const char *) n->name : "?");
            dump_node(n->children, depth + 1);
        } else if (n->type == XML_TEXT_NODE) {
            printf("%*stext[%s]\n", depth * 2, "", n->content ? (const char *) n->content : "");
        } else if (n->type == XML_DTD_NODE || n->type == XML_DOCUMENT_NODE ||
                   n->type == XML_HTML_DOCUMENT_NODE) {
            printf("%*s%s\n", depth * 2, "",
                   n->type == XML_DTD_NODE ? "DTD" : "doc");
            dump_node(n->children, depth + 1);
        } else {
            printf("%*snode type %d\n", depth * 2, "", n->type);
        }
    }
}

int main(int argc, char **argv) {
    const char *src = "<html>\n<head>\n<title>Hello</title>\n</head>\n<body>\nhi\n</body>\n</html>\n";
    htmlDocPtr d = htmlReadMemory(src, strlen(src), NULL, NULL,
                                  argc > 1 ? XML_PARSE_NOBLANKS : 0);
    dump_node((xmlNodePtr) d, 0);
    xmlFreeDoc(d);
    return 0;
}
