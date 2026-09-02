/* undecl-entity-probe.c — pin oracle (system libxml2 2.15.3) semantics for
 * undeclared general-entity references in content across DTD states. */
#include <stdio.h>
#include <libxml/parser.h>
#include <libxml/xmlerror.h>

static void run(const char *label, const char *doc) {
    xmlParserCtxtPtr ctxt = xmlNewParserCtxt();
    if (!ctxt) { printf("%s: no ctxt\n", label); return; }
    xmlDocPtr d = xmlCtxtReadMemory(ctxt, doc, (int)xmlStrlen((const xmlChar*)doc),
                               NULL, NULL, 0);
    printf("%s: read rc=%d wellFormed=%d errNo=%d doc=%s\n",
           label, d != NULL, ctxt->wellFormed, ctxt->errNo, d ? "yes" : "no");
    if (d) {
        /* Serialize children names to prove the parse continued past the ref */
        xmlNodePtr n = d->children;
        printf("%s: root=", label);
        while (n) {
            if (n->type == XML_ELEMENT_NODE)
                printf("<%s>", (const char*)n->name);
            n = n->next;
        }
        printf("\n");
        xmlFreeDoc(d);
    }
    xmlFreeParserCtxt(ctxt);
}

int main(void) {
    /* No DTD at all: undeclared reference is FATAL (WFC). */
    run("plain", "<root>a&nope;b</root>");
    /* Internal subset WITHOUT PE refs / external subset: still FATAL. */
    run("intsub", "<!DOCTYPE root [ <!ENTITY e \"E\"> ]><root>a&nope;b</root>");
    /* External subset declared (not loaded): non-fatal, parse continues. */
    run("extsub", "<!DOCTYPE root SYSTEM \"nope.dtd\"><root>a&nope;b</root>");
    /* Internal subset with a PE ref: non-fatal, parse continues. */
    run("peref", "<!DOCTYPE root [ <!ENTITY % p SYSTEM \"x\"> %p; ]><root>a&nope;b</root>");
    /* Both: non-fatal. */
    run("both", "<!DOCTYPE root SYSTEM \"nope.dtd\" [ <!ENTITY % p SYSTEM \"x\"> %p; ]><root>a&nope;b</root>");
    return 0;
}
