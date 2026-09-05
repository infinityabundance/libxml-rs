/*
 * distro-versioned-consumer.c — R-000179 distro-binary contract probe.
 *
 * Built against the VERSIONED distro libxml2 (libxml2.so.2, SONAME .2,
 * LIBXML2_2.x nodes — the host's /usr/lib/libxml2.so.2.13.9), exactly like a
 * distro-built binary. The linker records per-symbol LIBXML2_2.x version
 * requirements (DT_VERNEED on libxml2.so.2). Running the SAME binary against
 * an unversioned libxml2 makes ld.so print 'no version information
 * available'; the candidate's versioned profile (target/debug/versioned,
 * same SONAME + node graph) must bind every requirement silently and produce
 * byte-identical output to the distro run.
 *
 * Exercises a spread of introduction nodes: xmlReadMemory (LIBXML2_2.6.0),
 * xmlReadFile (LIBXML2_2.6.0), xmlReadDoc (2.6.0), xmlSaveFormatFileEnc
 * (2.4.30), xmlDocDumpFormatMemoryEnc (2.4.30), xmlXPathEvalExpression
 * (2.6.0), xmlGetCharEncodingName (2.4.30), xmlNodeDump (2.4.30), plus the
 * data symbol xmlParserVersion (2.4.30). Output is deterministic.
 */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>
#include <libxml/encoding.h>

static const char DOC[] = "<root a='1'><b>hello &amp; bye</b><c>2</c></root>";

int main(void) {
    int rc = 0;
    xmlInitParser();
    printf("version=%s\n", xmlParserVersion);
    xmlDocPtr doc = xmlReadMemory(DOC, (int) strlen(DOC), "t.xml", NULL,
                                  XML_PARSE_NOBLANKS);
    if (doc == NULL) { printf("readMemory-failed\n"); return 1; }
    xmlChar *mem = NULL;
    int size = 0;
    xmlDocDumpFormatMemoryEnc(doc, &mem, &size, "UTF-8", 0);
    if (mem == NULL) { printf("dump-failed\n"); xmlFreeDoc(doc); return 1; }
    printf("dump(%d)=%s", size, mem);
    xmlFree(mem);
    xmlFreeDoc(doc);

    /* XPath over a freshly parsed tree */
    doc = xmlReadFile("/etc/hostname", NULL, 0);
    if (doc != NULL) { xmlFreeDoc(doc); }
    doc = xmlParseDoc((const xmlChar *) "<x><y>3</y></x>");
    if (doc != NULL) {
        xmlXPathContextPtr ctx = xmlXPathNewContext(doc);
        xmlXPathObjectPtr obj =
            xmlXPathEvalExpression((const xmlChar *) "string(/x/y)", ctx);
        if (obj != NULL && obj->stringval != NULL) {
            printf("xpath=%s\n", obj->stringval);
            xmlXPathFreeObject(obj);
        }
        xmlXPathFreeContext(ctx);
        xmlFreeDoc(doc);
    }
    printf("enc=%s\n", xmlGetCharEncodingName(XML_CHAR_ENCODING_UTF8));
    xmlCleanupParser();
    printf("rc=%d\n", rc);
    return 0;
}
