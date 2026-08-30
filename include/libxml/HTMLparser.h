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










































































































































































































/* Deprecated default SAX v1 handler (globals.c 2.15.3). */
XMLPUBVAR const xmlSAXHandlerV1 htmlDefaultSAXHandler;








/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _htmlElemDesc htmlElemDesc;
typedef htmlElemDesc *htmlElemDescPtr;

typedef struct _htmlEntityDesc htmlEntityDesc;
typedef htmlEntityDesc *htmlEntityDescPtr;

typedef enum{
    HTML_PARSE_RECOVER = 1<<0,
    HTML_PARSE_NODEFDTD = 1<<2,
    HTML_PARSE_NOERROR = 1<<5,
    HTML_PARSE_NOWARNING = 1<<6,
    HTML_PARSE_PEDANTIC = 1<<7,
    HTML_PARSE_NOBLANKS = 1<<8,
    HTML_PARSE_NONET = 1<<11,
    HTML_PARSE_NOIMPLIED = 1<<13,
    HTML_PARSE_COMPACT = 1<<16,
    HTML_PARSE_HUGE = 1<<19,
    HTML_PARSE_IGNORE_ENC =1<<21,
    HTML_PARSE_BIG_LINES = 1<<22,
    HTML_PARSE_HTML5 = 1<<26
} htmlParserOption;

typedef enum{
  HTML_NA = 0 ,		/* something we don't check at all */
  HTML_INVALID = 0x1 ,
  HTML_DEPRECATED = 0x2 ,
  HTML_VALID = 0x4 ,
  HTML_REQUIRED = 0xc /* VALID bit set so ( & HTML_VALID ) is TRUE */
} htmlStatus;

struct _htmlElemDesc {
    const char *name;	/* The tag name */
    char startTag;      /* unused */
    char endTag;        /* Whether the end tag can be implied */
    char saveEndTag;    /* unused */
    char empty;         /* Is this an empty element ? */
    char depr;          /* unused */
    char dtd;           /* unused */
    char isinline;      /* is this a block 0 or inline 1 element */
    const char *desc;   /* the description */

    const char** subelts XML_DEPRECATED_MEMBER;
    const char* defaultsubelt XML_DEPRECATED_MEMBER;
    const char** attrs_opt XML_DEPRECATED_MEMBER;
    const char** attrs_depr XML_DEPRECATED_MEMBER;
    const char** attrs_req XML_DEPRECATED_MEMBER;

    int dataMode;
};

struct _htmlEntityDesc {
    unsigned int value;	/* the UNICODE value for the character */
    const char *name;	/* The entity name */
    const char *desc;   /* the description */
};

/* [11.1-G] end: extracted definitions */
#ifdef __cplusplus
}
#endif

#endif /* __HTML_PARSER_H__ */
