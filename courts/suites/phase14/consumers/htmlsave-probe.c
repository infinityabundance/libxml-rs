#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/HTMLparser.h>
#include <libxml/xmlsave.h>

typedef struct { char *buf; size_t len, cap; } sbuf;

static int wcb(void *ctx, const char *buffer, int len) {
    sbuf *s = ctx;
    if (s->len + len + 1 > s->cap) {
        s->cap = (s->len + len + 1) * 2;
        s->buf = realloc(s->buf, s->cap);
    }
    memcpy(s->buf + s->len, buffer, len);
    s->len += len;
    s->buf[s->len] = 0;
    return len;
}

int main(void) {
    const char *html = "<html><body><p>hi</p></body></html>";
    htmlDocPtr h = htmlReadMemory(html, strlen(html), NULL, NULL, 0);
    printf("standalone=%d properties=0x%x type=%d\n", h->standalone, h->properties, h->type);
    sbuf s = {0};
    xmlSaveCtxtPtr ctxt = xmlSaveToIO(wcb, NULL, &s, NULL, XML_SAVE_AS_XML);
    long status = xmlSaveDoc(ctxt, h);
    status |= xmlSaveClose(ctxt);
    printf("status=%ld\n--- saved ---\n%s", status, s.buf ? s.buf : "(null)");
    free(s.buf);
    xmlFreeDoc(h);
    return 0;
}
