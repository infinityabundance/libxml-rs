/* pushmarkup2-probe.c — KEY-2 content-`<!`-markup oracle parity probe. */
#include <stdio.h>
#include <string.h>
#include <libxml/parser.h>

static void try_content(const char *label, const char *body) {
    xmlParserCtxtPtr c = xmlCreatePushParserCtxt(NULL, NULL, NULL, 0, NULL);
    xmlCtxtUseOptions(c, XML_PARSE_IGNORE_ENC);
    xmlParseChunk(c, "<root>", 6, 0);
    xmlParseChunk(c, body, (int) strlen(body), 0);
    xmlParseChunk(c, "</root>", 7, 1);
    printf("%-9s wf=%d errNo=%d\n", label, c->wellFormed, c->errNo);
    xmlFreeParserCtxt(c);
}

int main(void) {
    try_content("ENTITY", "<!ENTITY foo \"content\">");
    try_content("DOCTYPE", "<!DOCTYPE html>");
    try_content("ELEMENT", "<!ELEMENT x EMPTY>");
    try_content("comment", "<!-- ok -->");
    try_content("cdata", "<![CDATA[ok]]>");
    try_content("text", "hello <b>w</b>");
    return 0;
}
