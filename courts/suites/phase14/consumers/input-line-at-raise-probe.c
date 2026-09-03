#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/parserInternals.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

/* not_well_formed2.xml (php ext/dom load_error2_gte2_12): for every error,
 * print xmlError.line (the error's own line) AND ctxt->input->line / col at
 * the moment the structured error callback fires. PHP's generic handler
 * prints ctxt->input->line, so the two must agree for a byte-exact phpt. */

static const char DOC[] =
    "<?xml version=\"1.0\" ?>\n"
    "<!-- AttValue: \" or ' expected -->\n"
    "<books>\n"
    " <book number=nine>\n"
    "  <title>The Grapes of Wrath</title>\n"
    "  <author>John Steinbeck</author>\n"
    " </book>\n"
    "</books>\n";

static void errfunc(void *u, const xmlError *e) {
    xmlParserCtxtPtr ctxt = (xmlParserCtxtPtr) u;
    (void) e;
    if (ctxt && ctxt->input) {
        printf("ERR code=%d errline=%d inputline=%d inputcol=%d\n",
               e->code, e->line, ctxt->input->line, ctxt->input->col);
    } else {
        printf("ERR code=%d errline=%d (no input)\n", e->code, e->line);
    }
}

int main(void) {
    xmlParserCtxtPtr ctxt = xmlCreateMemoryParserCtxt(DOC, (int) strlen(DOC));
    if (ctxt == NULL) { printf("no ctxt\n"); return 1; }
    xmlSetStructuredErrorFunc(ctxt, errfunc);
    xmlParseDocument(ctxt);
    printf("wellFormed=%d errNo=%d doc=%s\n", ctxt->wellFormed, ctxt->errNo,
           ctxt->myDoc ? "parsed" : "NULL");
    if (ctxt->myDoc) xmlFreeDoc(ctxt->myDoc);
    xmlFreeParserCtxt(ctxt);
    return 0;
}
