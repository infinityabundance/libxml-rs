#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxslt/xslt.h>
#include <libxslt/transform.h>
#include <libxslt/xsltInternals.h>

static const char *SCHEMA =
  "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\""
  " xmlns:sch=\"http://purl.oclc.org/dsdl/schematron\">"
  "<xs:element name=\"message\"><xs:complexType><xs:sequence>"
  "<xs:element name=\"number_of_entries\" type=\"xs:positiveInteger\">"
  "<xs:annotation><xs:appinfo><sch:pattern id=\"p\"><sch:title>t</sch:title>"
  "<sch:rule context=\"number_of_entries\"><sch:assert test=\"x\">E</sch:assert>"
  "</sch:rule></sch:pattern></xs:appinfo></xs:annotation></xs:element>"
  "</xs:sequence></xs:complexType></xs:element></xs:schema>";

int main(int argc, char **argv) {
    xsltStylesheetPtr st = xsltParseStylesheetFile((const xmlChar *)argv[1]);
    if (!st) { printf("stylesheet parse FAILED\n"); return 1; }
    xmlDocPtr doc;
    if (argc > 2 && argv[2][0]) {
        doc = xmlReadFile(argv[2], NULL, 0);
    } else {
        doc = xmlReadMemory(SCHEMA, (int)strlen(SCHEMA), "s", NULL, 0);
    }
    if (!doc) { printf("doc parse FAILED\n"); return 1; }
    xmlDocPtr res = xsltApplyStylesheet(st, doc, NULL);
    if (!res) { printf("transform FAILED\n"); return 1; }
    printf("res->children=%p res->last=%p", (void*)res->children, (void*)res->last);
    if (res->children) {
        printf(" root=%s", res->children->name ? (char*)res->children->name : "?");
    }
    xmlChar *out = NULL; int len = 0;
    xmlDocDumpMemory(res, &out, &len);
    printf(" outlen=%d\n---OUT---\n%.*s\n---END---\n", len, len, out ? (char*)out : "");
    if (out) xmlFree(out);
    return 0;
}
