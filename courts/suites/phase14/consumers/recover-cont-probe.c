#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

/* Recovery-continuation probe (php-14.3 E1 / dom loadXML_error1_gte2_12):
 * php-src/ext/dom/tests/not_well_formed.xml parsed through xmlReadMemory
 * with and without XML_PARSE_RECOVER. Upstream (2.15) keeps scanning after a
 * FATAL structural error — the end tag closes the CURRENT open element with
 * an "Opening and ending tag mismatch" report and the parse continues to the
 * end of the document, so BOTH mismatches are reported in one pass.
 *
 * Usage: ./x [recover]
 */

static const char DOC[] =
    "<?xml version=\"1.0\" ?>\n"
    "<!-- Opening and ending tag mismatch -->\n"
    "<books>\n"
    " <book>\n"
    "  <title>The Grapes of Wrath\n"
    "  <author>John Steinbeck</author>\n"
    " </book>\n"
    " <book>\n"
    "  <title>The Pearl</title>\n"
    "  <author>John Steinbeck</author>\n"
    " </book>\n"
    "</books>\n";

static int count = 0;

static void errfunc(void *ctx, const xmlError *e) {
    (void) ctx;
    count++;
    printf("err[%d] code=%d level=%d line=%d int2=%d\n", count, e->code,
           e->level, e->line, e->int2);
    printf("  str1=%s str2=%s\n",
           e->str1 ? (const char *) e->str1 : "(null)",
           e->str2 ? (const char *) e->str2 : "(null)");
    printf("  msg=%s", e->message ? (const char *) e->message : "(null)\n");
}

int main(int argc, char **argv) {
    int opts = 0;
    if ((argc > 1) && (strcmp(argv[1], "recover") == 0))
        opts = XML_PARSE_RECOVER;

    xmlSetStructuredErrorFunc(NULL, errfunc);
    xmlDocPtr d = xmlReadMemory(DOC, (int) strlen(DOC), "not_well_formed.xml",
                                NULL, opts);
    xmlSetStructuredErrorFunc(NULL, NULL);

    printf("total errors: %d\n", count);
    printf("doc=%s\n", d ? "parsed" : "NULL");
    if (d) {
        xmlNodePtr root = xmlDocGetRootElement(d);
        if (root)
            printf("root=%s\n", (const char *) root->name);
        xmlChar *s = NULL;
        xmlDocDumpFormatMemory(d, &s, NULL, 0);
        if (s) {
            printf("--- dump ---\n%s\n--- end ---\n", (const char *) s);
            xmlFree(s);
        }
        xmlFreeDoc(d);
    }
    return 0;
}
