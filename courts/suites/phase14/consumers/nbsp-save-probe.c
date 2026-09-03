#include <stdio.h>
#include <string.h>
#include <stdlib.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/HTMLparser.h>
#include <libxml/xmlsave.h>

typedef struct { char *buf; size_t len, cap; } sbuf;
static int wcb(void *ctx, const char *buffer, int len) {
    sbuf *s = ctx;
    if (s->len + len + 1 > s->cap) { s->cap = (s->len + len + 1) * 2; s->buf = realloc(s->buf, s->cap); }
    memcpy(s->buf + s->len, buffer, len); s->len += len; s->buf[s->len] = 0;
    return len;
}
static void saveasxml(xmlDocPtr d) {
    sbuf s = {0};
    xmlSaveCtxtPtr c = xmlSaveToIO(wcb, NULL, &s, NULL, XML_SAVE_AS_XML);
    xmlSaveDoc(c, d); xmlSaveClose(c);
    printf("--- as-xml ---\n%s", s.buf ? s.buf : "");
    free(s.buf);
}

int main(void) {
    const char *html = "<html><body><p>a&nbsp;b &#160; c</p></body></html>";
    htmlDocPtr h = htmlReadMemory(html, strlen(html), NULL, NULL, 0);
    saveasxml(h);
    xmlFreeDoc(h);
    return 0;
}
