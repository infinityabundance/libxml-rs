#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/HTMLparser.h>

int main(void) {
    const char *html = "<html><body><p>hi</p></body></html>";
    htmlDocPtr h = htmlReadMemory(html, strlen(html), NULL, NULL, 0);
    printf("doc standalone=%d properties=0x%x\n", h->standalone, h->properties);
    xmlFreeDoc(h);
    return 0;
}
