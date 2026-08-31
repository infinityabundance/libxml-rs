/*
 * test-libexslt.c — external C consumer probe for the 11.1-S build courts.
 *
 * Court family: BUILD-PKGCONFIG (11.1-S).
 *
 * Consumer program for the libexslt drop-in using standard tooling:
 *
 *   cc $(pkg-config --cflags libexslt) test-libexslt.c $(pkg-config --libs libexslt)
 *
 * pkg-config resolves the `Requires: libxml-2.0, libxslt` dependency chain
 * automatically (-lexslt -lxslt -lxml2), which exercises the 11.1-Z.1
 * three-DSO NEEDED chain (libexslt.so.0 -> libxslt.so.1 -> libxml2.so.16).
 */
#include <stdio.h>
#include <libexslt/exslt.h>

int main(void) {
    exsltRegisterAll();
    printf("exslt=%d xml=%d xslt=%d\n",
           exsltLibexsltVersion, exsltLibxmlVersion, exsltLibxsltVersion);
    return 0;
}
