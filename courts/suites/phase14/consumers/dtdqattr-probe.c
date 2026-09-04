#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

int main(void) {
    const char *src = "<!DOCTYPE root [<!ATTLIST root p:A CDATA #FIXED \"d\">]><root xmlns:p=\"urn:p\"/>";
    xmlDocPtr d = xmlReadMemory(src, strlen(src), NULL, NULL, 0);
    xmlDtdPtr intSubset = d->intSubset;
    xmlAttributePtr a1 = xmlGetDtdAttrDesc(intSubset, BAD_CAST "root", BAD_CAST "A");
    xmlAttributePtr a2 = xmlGetDtdAttrDesc(intSubset, BAD_CAST "root", BAD_CAST "p:A");
    xmlAttributePtr a3 = xmlGetDtdQAttrDesc(intSubset, BAD_CAST "root", BAD_CAST "A", BAD_CAST "p");
    xmlAttributePtr a4 = xmlGetDtdQAttrDesc(intSubset, BAD_CAST "root", BAD_CAST "A", NULL);
    printf("attrdesc(root,A)=%p attrdesc(root,p:A)=%p\n", (void *) a1, (void *) a2);
    printf("qattrdesc(root,A,p)=%p qattrdesc(root,A,NULL)=%p\n", (void *) a3, (void *) a4);
    if (a1) printf("a1 name=%s prefix=%s elem=%s def=%s\n", (char *) a1->name, a1->prefix ? (char *) a1->prefix : "(null)", (char *) a1->elem, a1->defaultValue ? (char *) a1->defaultValue : "(null)");
    /* What decls exist at all? dump via the element decl's attributes? Walk dtd->attributes table is opaque; use hasProp on the element */
    xmlNodePtr root = xmlDocGetRootElement(d);
    xmlAttrPtr h1 = xmlHasNsProp(root, BAD_CAST "A", BAD_CAST "urn:p");
    printf("hasNsProp(A,urn:p)=%p type=%d\n", (void *) h1, h1 ? h1->type : -1);
    xmlAttrPtr h2 = xmlHasNsProp(root, BAD_CAST "A", NULL);
    printf("hasNsProp(A,NULL)=%p type=%d\n", (void *) h2, h2 ? h2->type : -1);
    xmlChar *mem = NULL; int size = 0;
    xmlDocDumpMemory(d, &mem, &size);
    printf("--- doc ---\n%.*s\n", size, mem ? (char *) mem : "");
    xmlFree(mem);
    xmlFreeDoc(d);
    return 0;
}
