/* ctxoffset-probe.c — dump offsetof() for the parser-context fields that
 * PHP expat-compat compat.c reads at runtime. Compiled against the real
 * headers AND against the candidate headers; the outputs must match. */
#include <stdio.h>
#include <stddef.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/entities.h>

int main(void) {
    printf("sizeof(xmlParserCtxt)=%zu\n", sizeof(xmlParserCtxt));
    printf("offsetof(sax)=%zu\n", offsetof(xmlParserCtxt, sax));
    printf("offsetof(userData)=%zu\n", offsetof(xmlParserCtxt, userData));
    printf("offsetof(myDoc)=%zu\n", offsetof(xmlParserCtxt, myDoc));
    printf("offsetof(wellFormed)=%zu\n", offsetof(xmlParserCtxt, wellFormed));
    printf("offsetof(errNo)=%zu\n", offsetof(xmlParserCtxt, errNo));
    printf("offsetof(inSubset)=%zu\n", offsetof(xmlParserCtxt, inSubset));
    printf("offsetof(instate)=%zu\n", offsetof(xmlParserCtxt, instate));
    printf("offsetof(disableSAX)=%zu\n", offsetof(xmlParserCtxt, disableSAX));
    printf("offsetof(input)=%zu\n", offsetof(xmlParserCtxt, input));
    printf("offsetof(inputNr)=%zu\n", offsetof(xmlParserCtxt, inputNr));
    printf("sizeof(xmlEntity)=%zu\n", sizeof(xmlEntity));
    printf("offsetof(xmlEntity,name)=%zu\n", offsetof(xmlEntity, name));
    printf("offsetof(xmlEntity,etype)=%zu\n", offsetof(xmlEntity, etype));
    printf("offsetof(xmlEntity,content)=%zu\n", offsetof(xmlEntity, content));
    printf("offsetof(xmlEntity,SystemID)=%zu\n", offsetof(xmlEntity, SystemID));
    printf("offsetof(xmlEntity,ExternalID)=%zu\n", offsetof(xmlEntity, ExternalID));
    printf("sizeof(xmlSAXHandler)=%zu\n", sizeof(xmlSAXHandler));
    printf("offsetof(sax,getEntity)=%zu\n", offsetof(xmlSAXHandler, getEntity));
    printf("offsetof(sax,startElement)=%zu\n", offsetof(xmlSAXHandler, startElement));
    printf("offsetof(sax,initialized)=%zu\n", offsetof(xmlSAXHandler, initialized));
    printf("offsetof(sax,startElementNs)=%zu\n", offsetof(xmlSAXHandler, startElementNs));
    return 0;
}
