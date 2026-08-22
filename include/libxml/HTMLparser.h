/**
 * @file
 *
 * HTML parser API for libxml-rs
 */

#ifndef __HTML_PARSER_H__
#define __HTML_PARSER_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/parser.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef xmlDocPtr htmlDocPtr;

XMLPUBFUN htmlDocPtr htmlParseFile(const char *filename, const char *encoding);
XMLPUBFUN htmlDocPtr htmlParseMemory(const char *buffer, int size);
XMLPUBFUN htmlDocPtr htmlParseDoc(const xmlChar *cur, const char *encoding);
XMLPUBFUN void *htmlCreateFileParserCtxt(const char *filename, const char *encoding);
XMLPUBFUN void htmlFreeParserCtxt(void *ctxt);
XMLPUBFUN void htmlInitParser(void);
XMLPUBFUN void htmlCleanupParser(void);

#ifdef __cplusplus
}
#endif

#endif /* __HTML_PARSER_H__ */
