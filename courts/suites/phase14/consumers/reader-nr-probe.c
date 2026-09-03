#include <stdio.h>
#include <string.h>
#include <libxml/xmlreader.h>
#include <libxml/xmlIO.h>

/* NR isolation: which reader constructor shapes yield events? Each probe
 * walks a small doc and counts nodeType:name events. */

static const char DOC[] =
    "<?xml version=\"1.0\"?>\n<books><book num=\"1\"><title>T</title></book></books>";

static int walk(xmlTextReaderPtr r) {
    if (r == NULL) { printf("  reader=NULL\n"); return -1; }
    int n = 0, ret;
    while ((ret = xmlTextReaderRead(r)) == 1) {
        printf("  %d:%s\n", xmlTextReaderNodeType(r),
               xmlTextReaderConstName(r) ? (const char *) xmlTextReaderConstName(r) : "(no name)");
        n++;
    }
    printf("  events=%d read-ret=%d\n", n, ret);
    return n;
}

static int io_read(void *ctx, char *buf, int len) {
    (void) ctx;
    static const char *data = DOC;
    static int pos = 0;
    int remaining = (int) strlen(data) - pos;
    if (remaining <= 0) return 0;
    if (len > remaining) len = remaining;
    memcpy(buf, data + pos, len);
    pos += len;
    return len;
}

int main(void) {
    printf("== xmlReaderForFile ==\n");
    FILE *f = fopen("/tmp/nr-doc.xml", "w");
    fputs(DOC, f);
    fclose(f);
    walk(xmlReaderForFile("/tmp/nr-doc.xml", NULL, 0));

    printf("== xmlReaderForMemory ==\n");
    walk(xmlReaderForMemory(DOC, (int) strlen(DOC), "mem.xml", NULL, 0));

    printf("== xmlReaderForIO ==\n");
    walk(xmlReaderForIO(io_read, NULL, NULL, "io.xml", NULL, 0));

    printf("== xmlNewTextReader + xmlTextReaderSetup (mem input) ==\n");
    {
        xmlParserInputBufferPtr ib = xmlParserInputBufferCreateMem(DOC, (int) strlen(DOC),
                                                                   XML_CHAR_ENCODING_NONE);
        if (ib == NULL) { printf("  no input buffer\n"); return 1; }
        xmlTextReaderPtr r = xmlNewTextReader(ib, "newt.xml");
        int rc = xmlTextReaderSetup(r, NULL, "newt.xml", NULL, 0);
        printf("  setup rc=%d\n", rc);
        walk(r);
    }
    return 0;
}
