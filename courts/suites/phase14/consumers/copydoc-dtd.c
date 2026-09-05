#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
int main(void) {
    xmlInitParser();
    const char *x = "<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\"><html><body/></html>";
    xmlDocPtr doc = xmlReadMemory(x, (int)strlen(x), NULL, NULL, 0);
    if (!doc) { printf("read fail\n"); return 1; }
    printf("intSubset=%p\n", (void*)doc->intSubset);
    xmlNodePtr ch = doc->children;
    int i = 0;
    while (ch) { printf("child[%d] type=%d name=%s\n", i, ch->type, ch->name ? (char*)ch->name : "(null)"); ch = ch->next; i++; }
    xmlFreeDoc(doc);
    return 0;
}
