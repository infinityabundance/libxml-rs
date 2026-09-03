#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/HTMLparser.h>

/* What exactly does the html parser store for &nbsp; / &#160; and how does
 * the xml serializer escape it? */
int main(void) {
    const char *html = "<html><body><p>a&nbsp;b&#160;c</p></body></html>";
    htmlDocPtr h = htmlReadMemory(html, strlen(html), NULL, NULL, 0);
    xmlNodePtr p = h->children;                 /* html */
    for (; p && p->type != XML_ELEMENT_NODE; p = p->next) {}
    xmlNodePtr body = p ? p->children : NULL;
    for (; body && body->type != XML_ELEMENT_NODE; body = body->next) {}
    xmlNodePtr par = body ? body->children : NULL;
    for (; par && par->type != XML_ELEMENT_NODE; par = par->next) {}
    xmlNodePtr txt = par ? par->children : NULL;
    if (txt && txt->type == XML_TEXT_NODE) {
        printf("text len=%d bytes:", txt->content ? (int) strlen((char *) txt->content) : -1);
        if (txt->content) {
            unsigned char *c = (unsigned char *) txt->content;
            for (int i = 0; c[i]; i++) printf(" %02x", c[i]);
        }
        printf("\n");
    } else {
        printf("no text child; txt=%p\n", (void *) txt);
    }
    xmlFreeDoc(h);
    return 0;
}
