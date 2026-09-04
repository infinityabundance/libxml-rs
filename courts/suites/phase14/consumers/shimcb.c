#define _GNU_SOURCE
#include <stdio.h>
#include <dlfcn.h>
typedef int (*write_cb_t)(void*, const char*, int);
typedef void* (*save_io_fn)(write_cb_t, void*, void*, const char*, int);
typedef int (*flush_fn)(void*);
typedef int (*close_fn)(void*);
typedef long (*save_doc_fn)(void*, void*);
typedef int (*save_close_fn)(void*);
static write_cb_t real_io_fn;
static void* real_ctx;
static save_io_fn real_save_io;
static flush_fn real_flush;
static close_fn real_obuf_close;
static save_doc_fn real_save_doc;
static save_close_fn real_save_close;
static int wrap_write(void* ctx, const char* buf, int len) {
    fprintf(stderr, "SHIMCALLBACK len=%d\n", len);
    return real_io_fn(real_ctx, buf, len);
}
void* xmlSaveToIO(void* w, void* c, void* ctx, const char* enc, int opt) {
    if (!real_save_io) real_save_io = (save_io_fn)dlsym(RTLD_NEXT, "xmlSaveToIO");
    real_io_fn = (write_cb_t)w;
    real_ctx = ctx;
    fprintf(stderr, "SHIM xmlSaveToIO opt=0x%x\n", opt);
    return real_save_io(wrap_write, c, ctx, enc, opt);
}
int xmlOutputBufferFlush(void* out) {
    if (!real_flush) real_flush = (flush_fn)dlsym(RTLD_NEXT, "xmlOutputBufferFlush");
    int r = real_flush(out);
    fprintf(stderr, "SHIM xmlOutputBufferFlush -> %d\n", r);
    return r;
}
int xmlOutputBufferClose(void* out) {
    if (!real_obuf_close) real_obuf_close = (close_fn)dlsym(RTLD_NEXT, "xmlOutputBufferClose");
    int r = real_obuf_close(out);
    fprintf(stderr, "SHIM xmlOutputBufferClose -> %d\n", r);
    return r;
}
