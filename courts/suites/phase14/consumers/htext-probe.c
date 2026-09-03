#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/HTMLparser.h>
#include <libxml/HTMLtree.h>

static void find_text(xmlNodePtr n, int depth) {
    for (; n; n = n->next) {
        if (n->type == XML_TEXT_NODE) {
            printf("text depth=%d name=%s hex=", depth,
                   n->name ? (const char *) n->name : "(null)");
            unsigned char *c = (unsigned char *) n->content;
            for (int i = 0; c && c[i]; i++) printf("%02x", c[i]);
            printf(" rawchildren=%p\n", (void *) n->children);
        } else if (n->type == XML_ELEMENT_NODE) {
            find_text(n->children, depth + 1);
        }
    }
}

int main(int argc, char **argv) {
    const char *src = "<html><body><p>a&nbsp;b&#160;c&eacute;d</p></body></html>";
    htmlDocPtr d = htmlReadMemory(src, strlen(src), NULL, NULL, 0);
    find_text((xmlNodePtr) d, 0);
    xmlFreeDoc(d);
    return 0;
}
