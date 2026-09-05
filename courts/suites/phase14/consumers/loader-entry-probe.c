#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlreader.h>

static int loader_seen = 0;
static xmlParserInputPtr my_loader(const char *URL, const char *ID,
                                   xmlParserCtxtPtr ctxt) {
    (void) ID; (void) ctxt;
    loader_seen++;
    return xmlNoNetExternalEntityLoader(URL, ID, ctxt);
}

static void tick(const char *name) {
    printf("%s=%d\n", name, loader_seen);
    loader_seen = 0;
}

int main(void) {
    FILE *fp = fopen("t.xml", "w");
    if (fp) { fputs("<a><b>hi</b></a>", fp); fclose(fp); }

    xmlSetExternalEntityLoader(my_loader);

    xmlDocPtr d;
    d = xmlReadFile("t.xml", NULL, 0);            if (d) xmlFreeDoc(d);    tick("xmlReadFile");
    d = xmlReadMemory("<x/>", 4, "mem.xml", NULL, 0); if (d) xmlFreeDoc(d); tick("xmlReadMemory");

    xmlParserCtxtPtr c = xmlNewParserCtxt();
    d = xmlCtxtReadFile(c, "t.xml", NULL, 0);     if (d) xmlFreeDoc(d);    tick("xmlCtxtReadFile");
    xmlFreeParserCtxt(c);

    extern xmlParserCtxtPtr xmlCreateURLParserCtxt(const char *, int);
    c = xmlCreateURLParserCtxt("t.xml", 0);       if (c) { xmlParseDocument(c); xmlFreeParserCtxt(c); } tick("xmlCreateURLParserCtxt");
    extern xmlParserCtxtPtr xmlCreateFileParserCtxt(const char *);
    c = xmlCreateFileParserCtxt("t.xml");

         if (c) { xmlParseDocument(c); xmlFreeParserCtxt(c); } tick("xmlCreateFileParserCtxt");

    d = xmlParseFile("t.xml");                    if (d) xmlFreeDoc(d);    tick("xmlParseFile");

    xmlSAXHandler h; memset(&h, 0, sizeof(h)); xmlSAXVersion(&h, 2);
    d = xmlSAXParseFile(&h, "t.xml", 0);          if (d) xmlFreeDoc(d);    tick("xmlSAXParseFile");

    xmlTextReaderPtr r = xmlReaderForFile("t.xml", NULL, 0);
    if (r) { while (xmlTextReaderRead(r) > 0) {} xmlFreeTextReader(r); }
    tick("xmlReaderForFile");

    /* DTD + external entity parse */
    xmlDtdPtr dtd = xmlParseDTD(NULL, "t.xml");
    if (dtd) { }
    tick("xmlParseDTD");

    remove("t.xml");
    return 0;
}
