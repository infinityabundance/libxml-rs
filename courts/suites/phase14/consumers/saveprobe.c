#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef struct { char* s; size_t a; } smart_str;
extern void* xmlNewDoc(const char* version);
extern void* xmlSaveToIO(void* write_cb, void* close_cb, void* ctx, const char* encoding, int options);
extern long xmlSaveDoc(void* ctxt, void* doc);
extern int xmlSaveClose(void* ctxt);
static int my_write(void* ctx, const char* buf, int len) {
    smart_str* s = (smart_str*)ctx;
    fprintf(stderr, "CALLBACK len=%d buf=[%.*s]\n", len, len, buf);
    if (s->s == NULL) { s->s = malloc(len + 1); s->a = len + 1; }
    memcpy(s->s + strlen(s->s), buf, len);
    s->s[strlen(s->s) + len] = 0;
    return len;
}
int main(void) {
    fprintf(stderr, "start\n");
    void* doc = xmlNewDoc(NULL);
    smart_str str; memset(&str, 0, sizeof(str));
    void* ctxt = xmlSaveToIO((void*)my_write, NULL, &str, NULL, 0x22);
    fprintf(stderr, "ctxt=%p\n", ctxt);
    long st = xmlSaveDoc(ctxt, doc);
    fprintf(stderr, "saveDoc=%ld str.s=%p\n", st, str.s);
    int cl = xmlSaveClose(ctxt);
    fprintf(stderr, "saveClose=%d str.s=%p content=[%s]\n", cl, str.s, str.s ? str.s : "(null)");
    return 0;
}
