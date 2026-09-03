#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/HTMLparser.h>
#include <libxml/HTMLtree.h>

int main(void) {
    static const unsigned char src[] =
        "<html><body><p>a\xc2\xa0"
        "b\xc3\xa9"
        "\xe2\x98\x83"
        "c</p></body></html>";
    htmlDocPtr d = htmlReadMemory((const char *) src, sizeof(src) - 1, NULL, NULL, 0);
    xmlChar *mem = NULL;
    int size = 0;
    htmlDocDumpMemoryFormat(d, &mem, &size, 0);
    printf("bytes: ");
    if (mem) {
        unsigned char *b = (unsigned char *) mem;
        for (int i = 0; i < size; i++) {
            if (b[i] >= 0x20 && b[i] < 0x7f) printf("%c", b[i]);
            else printf("\\x%02x", b[i]);
        }
    }
    printf("\n");
    if (mem) xmlFree(mem);
    xmlFreeDoc(d);
    return 0;
}
