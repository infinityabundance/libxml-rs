#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>

/* Mirror php dom_set_attribute_ns_modern: xmlSetNsProp(elem, ns, localname,
 * value) where ns = {prefix=foo, href=http://php.net/2} while the element
 * already binds foo -> http://php.net. What does the engine serialize? */
int main(void) {
    const char *src =
        "<?xml version=\"1.0\"?><container xmlns:foo=\"http://php.net\" foo:bar=\"yes\"/>";
    xmlDocPtr d = xmlReadMemory(src, strlen(src), NULL, NULL, XML_PARSE_NSCLEAN);
    xmlNodePtr el = xmlDocGetRootElement(d);

    /* synthetic php-style owned ns */
    xmlNsPtr ns = xmlMalloc(sizeof(*ns));
    memset(ns, 0, sizeof(*ns));
    ns->type = XML_LOCAL_NAMESPACE;
    ns->prefix = BAD_CAST "foo";
    ns->href = BAD_CAST "http://php.net/2";

    xmlAttrPtr a = xmlSetNsProp(el, ns, BAD_CAST "bar", BAD_CAST "no1");
    printf("attr=%p\n", (void *) a);
    xmlDocDumpMemory(d, NULL, NULL);

    /* Then a second, prefix-less one for the same URI (cached ns) */
    xmlAttrPtr a2 = xmlSetNsProp(el, ns, BAD_CAST "baz", BAD_CAST "no2");
    printf("attr2=%p\n", (void *) a2);
    xmlChar *mem = NULL;
    int size = 0;
    xmlDocDumpMemory(d, &mem, &size);
    printf("--- serialized ---\n%.*s\n", size, mem ? (char *) mem : "");
    xmlFree(mem);
    xmlFreeDoc(d);
    return 0;
}
