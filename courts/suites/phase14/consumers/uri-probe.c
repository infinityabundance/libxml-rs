#include <stdio.h>
#include <string.h>
#include <libxml/uri.h>
#include <libxml/xmlstring.h>
#include <libxml/xmlmemory.h>

int main(void) {
    const char *paths[] = {"abc.xml", "-", "/tmp/zz-abs-004.xml", "php://memory", "004.xml", NULL};
    for (int i = 0; paths[i]; i++) {
        xmlURIPtr uri = xmlParseURI(paths[i]);
        printf("xmlParseURI(%s) -> %s\n", paths[i], uri ? "URI" : "NULL");
        if (uri) {
            printf("   scheme=%s path=%s\n", uri->scheme ? (const char*)uri->scheme : "(null)",
                   uri->path ? (const char*)uri->path : "(null)");
            xmlFreeURI(uri);
        }
        xmlChar *unesc = xmlURIUnescapeString(paths[i], 0, NULL);
        printf("   xmlURIUnescapeString -> [%s]%s\n", unesc ? (const char*)unesc : "(null)",
               unesc && *unesc == 0 ? "  <EMPTY!>" : "");
        if (unesc) xmlFree(unesc);
        xmlChar *esc = xmlURIEscapeStr((const xmlChar*)paths[i], (const xmlChar*)":");
        printf("   xmlURIEscapeStr -> [%s]\n", esc ? (const char*)esc : "(null)");
        if (esc) xmlFree(esc);
    }
    return 0;
}
