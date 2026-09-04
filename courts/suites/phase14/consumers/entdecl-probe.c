#define _GNU_SOURCE
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
int main(void) {
    const char* xml = "<?xml version=\"1.0\"?>\n"
        "<!DOCTYPE root [\n"
        "<!ENTITY ent \"foo\">\n"
        "<!ENTITY test \"entity is only for test purposes\">\n"
        "]>\n"
        "<root a=\"x&ent;x\"><div>&test;</div></root>";
    xmlDocPtr doc = xmlReadMemory(xml, strlen(xml), NULL, NULL, 0);
    xmlNodePtr root = xmlDocGetRootElement(doc);
    xmlAttrPtr a = root->properties;
    printf("attr a children: ");
    for (xmlNodePtr c = a->children; c; c = c->next) printf("[%d %s] ", c->type, c->name);
    printf("\n");
    xmlChar* v = xmlNodeGetContent((xmlNode*)a);
    printf("xmlNodeGetContent(attr)=[%s]\n", v ? (char*)v : "(null)");
    if (v) xmlFree(v);
    // entity decls
    xmlEntityPtr ent = xmlGetDocEntity(doc, BAD_CAST "ent");
    printf("ent: children=");
    for (xmlNodePtr c = ent->children; c; c = c->next) printf("[%d %s content=%s] ", c->type, c->name, c->content ? (char*)c->content : "(null)");
    printf(" content=[%s]\n", ent->content ? (char*)ent->content : "(null)");
    xmlEntityPtr test = xmlGetDocEntity(doc, BAD_CAST "test");
    printf("test: children=");
    for (xmlNodePtr c = test->children; c; c = c->next) printf("[%d %s content=%s] ", c->type, c->name, c->content ? (char*)c->content : "(null)");
    printf(" content=[%s]\n", test->content ? (char*)test->content : "(null)");
    // ref node inside div
    xmlNodePtr div = root->children;
    printf("div children: ");
    for (xmlNodePtr c = div->children; c; c = c->next) printf("[%d %s] ", c->type, c->name);
    printf("\n");
    xmlNodePtr ref = div->children; // should be the entity ref
    if (ref && ref->type == XML_ENTITY_REF_NODE) {
        xmlChar* rv = xmlNodeGetContent(ref);
        printf("xmlNodeGetContent(ref test)=[%s] ref->children=%p ref->content=%s\n", rv ? (char*)rv : "(null)", (void*)ref->children, ref->content ? (char*)ref->content : "(null)");
        if (rv) xmlFree(rv);
        xmlChar* rv2 = xmlNodeListGetString(doc, a->children, 1);
        printf("xmlNodeListGetString(attr children, inLine=1)=[%s]\n", rv2 ? (char*)rv2 : "(null)");
        if (rv2) xmlFree(rv2);
        xmlChar* rv3 = xmlNodeListGetString(NULL, (xmlNode*)ent, 1);
        printf("xmlNodeListGetString(NULL, ent, 1)=[%s]\n", rv3 ? (char*)rv3 : "(null)");
        if (rv3) xmlFree(rv3);
    }
    xmlFreeDoc(doc);
    return 0;
}
