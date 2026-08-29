/*
 * CATALOG-001 — differential probe of the xmlACatalog* / xmlCatalog* family
 * (11.1-I catalog closure).
 *
 * Compiled twice (oracle DSO vs candidate DSO); output must be byte-identical.
 * Semantics verified against the system libxml2 2.15.3:
 *   - a fresh xmlNewCatalog(0) shell rejects xmlACatalogAdd (-1) and
 *     resolves nothing;
 *   - xmlLoadACatalog parses entries (resolvable) but xmlCatalogIsEmpty
 *     reports 1 until an API add populates the children list;
 *   - adds after load work and flip isEmpty to 0.
 */
#include <stdio.h>
#include <string.h>
#include <libxml/catalog.h>
#include <libxml/tree.h>

static void p(const char *name, const xmlChar *v) {
    printf("%s=", name);
    if (v == NULL) { printf("(null)\n"); return; }
    printf("%s\n", (const char *) v);
}

int main(void) {
    /* 1. fresh shell behavior */
    xmlCatalogPtr h = xmlNewCatalog(0);
    printf("shell empty=%d\n", xmlCatalogIsEmpty(h));
    printf("shell add-ret=%d\n", xmlACatalogAdd(h, (const xmlChar *) "system",
        (const xmlChar *) "http://x/y", (const xmlChar *) "file:///y"));
    p("shell resolve", xmlACatalogResolveSystem(h, (const xmlChar *) "http://x/y"));
    xmlFreeCatalog(h);

    /* 2. load a real catalog file */
    FILE *f = fopen("/tmp/libxml-rs-cat-probe.xml", "w");
    fputs("<?xml version=\"1.0\"?>\n"
          "<catalog xmlns=\"urn:oasis:names:tc:entity:xmlns:xml:catalog\">\n"
          "  <public publicId=\"-//OASIS//DTD A//EN\" uri=\"file:///dtd/a.dtd\"/>\n"
          "  <system systemId=\"http://ex.com/x\" uri=\"file:///tmp/x.xml\"/>\n"
          "</catalog>\n", f);
    fclose(f);
    h = xmlLoadACatalog("/tmp/libxml-rs-cat-probe.xml");
    printf("load=%s\n", h ? "ok" : "null");
    printf("after-load empty=%d\n", h ? xmlCatalogIsEmpty(h) : -1);
    p("resolvePublic", xmlACatalogResolvePublic(h, (const xmlChar *) "-//OASIS//DTD A//EN"));
    p("resolveSystem", xmlACatalogResolveSystem(h, (const xmlChar *) "http://ex.com/x"));
    p("resolveMiss", xmlACatalogResolve(h, (const xmlChar *) "-//NOPE//EN", NULL));
    p("resolveBoth", xmlACatalogResolve(h, (const xmlChar *) "-//OASIS//DTD A//EN",
                                        (const xmlChar *) "http://ex.com/x"));

    /* 3. add after load */
    printf("add-ret=%d\n", xmlACatalogAdd(h, (const xmlChar *) "rewriteSystem",
        (const xmlChar *) "http://old/", (const xmlChar *) "http://new/"));
    printf("after-add empty=%d\n", xmlCatalogIsEmpty(h));
    p("resolveRewritten", xmlACatalogResolveSystem(h, (const xmlChar *) "http://old/a.xml"));
    printf("remove-ret=%d\n", xmlACatalogRemove(h, (const xmlChar *) "http://old/"));
    printf("remove-miss=%d\n", xmlACatalogRemove(h, (const xmlChar *) "http://old/"));

    /* 4. dump */
    FILE *tf = tmpfile();
    xmlACatalogDump(h, tf);
    fflush(tf);
    rewind(tf);
    char buf[4096];
    size_t n = fread(buf, 1, sizeof(buf) - 1, tf);
    buf[n] = 0;
    fclose(tf);
    printf("dump[%zu]=%s", n, buf);

    xmlFreeCatalog(h);
    remove("/tmp/libxml-rs-cat-probe.xml");

    /* 5. local catalog API + setters */
    void *local = NULL;
    printf("local-miss=%d\n", xmlCatalogLocalResolve(local, NULL, (const xmlChar *) "x") == NULL);
    xmlCatalogFreeLocal(local);
    printf("prefer-return=%d\n", xmlCatalogSetDefaultPrefer(XML_CATA_PREFER_SYSTEM));
    printf("debug=%d\n", xmlCatalogSetDebug(3));
    return 0;
}
