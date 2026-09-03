#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/xmlerror.h>

int main(void) {
    const char *doc = "<root><child/>";
    /* Capture what the generic error handler emits for a RECOVER parse of an
     * unterminated root. */
    xmlDocPtr d = xmlReadMemory(doc, (int) strlen(doc), "probe.xml", NULL,
                                XML_PARSE_RECOVER);
    if (d == NULL) {
        printf("== parse failed ==\n");
    } else {
        xmlChar *s;
        xmlDocDumpFormatMemory(d, &s, NULL, 0);
        printf("== parsed doc ==\n%s\n", (const char *) s);
        xmlFree(s);
        xmlFreeDoc(d);
    }
    printf("== last error ==\n");
    const xmlError *e = xmlGetLastError();
    if (e) {
        printf("code=%d level=%d line=%d msg=%s\n", e->code, e->level, e->line,
               e->message ? e->message : "(null)");
        printf("file=%s str1=%s str2=%s int1=%d int2=%d\n",
               e->file ? e->file : "(null)",
               e->str1 ? e->str1 : "(null)", e->str2 ? e->str2 : "(null)",
               e->int1, e->int2);
    }
    return 0;
}
