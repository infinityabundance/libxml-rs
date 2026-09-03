#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlsave.h>
typedef struct { char *buf; size_t len, cap; } sbuf;
static int wcb(void *ctx, const char *buffer, int len) {
    sbuf *s = ctx;
    if (s->len + len + 1 > s->cap) { s->cap = (s->len + len + 1) * 2; s->buf = realloc(s->buf, s->cap); }
    memcpy(s->buf + s->len, buffer, len); s->len += len; s->buf[s->len] = 0;
    return len;
}
int main(void) {
    const char *src = "<?xml version=\"1.0\"?><r>caf\xc3\xa9 \xc2\xa0 end</r>";
    xmlDocPtr d = xmlReadMemory(src, strlen(src), NULL, NULL, 0);
    sbuf s = {0};
    xmlSaveCtxtPtr c = xmlSaveToIO(wcb, NULL, &s, NULL, 0);
    xmlSaveDoc(c, d);
    xmlSaveClose(c);
    printf("no-enc save: %s\n", s.buf ? s.buf : "(null)");
    free(s.buf);
    xmlFreeDoc(d);
    return 0;
}
