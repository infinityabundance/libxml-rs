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
int main(void) {
    /* <html><body><p>a&lt;nbsp&gt;b&eacute;&#x2603;c</p></body></html> with
     * entities written as chars: U+00A0, U+00E9, U+2603 in UTF-8 */
    static const unsigned char src[] =
        "<html><body><p>a\xc2\xa0"
        "b\xc3\xa9"
        "\xe2\x98\x83"
        "c</p></body></html>";
    htmlDocPtr d = htmlReadMemory((const char *) src, sizeof(src) - 1, NULL, NULL, 0);
    sbuf s = {0};
    xmlSaveCtxtPtr c = xmlSaveToIO(wcb, NULL, &s, NULL, XML_SAVE_AS_HTML);
    xmlSaveDoc(c, d);
    xmlSaveClose(c);
    printf("html save bytes: ");
    if (s.buf) {
        unsigned char *b = (unsigned char *) s.buf;
        size_t i = 0;
        while (b[i]) {
            if (b[i] >= 0x20 && b[i] < 0x7f) printf("%c", b[i]);
            else printf("\\x%02x", b[i]);
            i++;
        }
    }
    printf("\n");
    free(s.buf);
    xmlFreeDoc(d);
    return 0;
}
