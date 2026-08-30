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

/* htmlParserCtxt aliases the XML parser context (upstream HTMLparser.h) */
typedef xmlParserCtxt htmlParserCtxt;
typedef xmlParserCtxtPtr htmlParserCtxtPtr;
typedef xmlSAXHandler htmlSAXHandler;
typedef xmlSAXHandlerPtr htmlSAXHandlerPtr;

































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

/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN htmlStatus htmlAttrAllowed(const htmlElemDesc*, const xmlChar*, int) ;
XMLPUBFUN int htmlAutoCloseTag(xmlDoc *doc, const xmlChar *name, xmlNode *elem);
XMLPUBFUN htmlParserCtxt * htmlCreateMemoryParserCtxt(const char *buffer, int size);
XMLPUBFUN htmlParserCtxt * htmlCreatePushParserCtxt(htmlSAXHandler *sax, void *user_data, const char *chunk, int size, const char *filename, xmlCharEncoding enc);
XMLPUBFUN xmlDoc * htmlCtxtParseDocument (htmlParserCtxt *ctxt, xmlParserInput *input);
XMLPUBFUN xmlDoc * htmlCtxtReadDoc (xmlParserCtxt *ctxt, const xmlChar *cur, const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDoc * htmlCtxtReadFd (xmlParserCtxt *ctxt, int fd, const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDoc * htmlCtxtReadFile (xmlParserCtxt *ctxt, const char *filename, const char *encoding, int options);
XMLPUBFUN xmlDoc * htmlCtxtReadIO (xmlParserCtxt *ctxt, xmlInputReadCallback ioread, xmlInputCloseCallback ioclose, void *ioctx, const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDoc * htmlCtxtReadMemory (xmlParserCtxt *ctxt, const char *buffer, int size, const char *URL, const char *encoding, int options);
XMLPUBFUN void htmlCtxtReset (htmlParserCtxt *ctxt);
XMLPUBFUN int htmlCtxtSetOptions (htmlParserCtxt *ctxt, int options);
XMLPUBFUN int htmlCtxtUseOptions (htmlParserCtxt *ctxt, int options);
XMLPUBFUN int htmlElementAllowedHere(const htmlElemDesc*, const xmlChar*) ;
XMLPUBFUN htmlStatus htmlElementStatusHere(const htmlElemDesc*, const htmlElemDesc*) ;
XMLPUBFUN int htmlEncodeEntities(unsigned char *out, int *outlen, const unsigned char *in, int *inlen, int quoteChar);
XMLPUBFUN const htmlEntityDesc * htmlEntityLookup(const xmlChar *name);
XMLPUBFUN const htmlEntityDesc * htmlEntityValueLookup(unsigned int value);
XMLPUBFUN int htmlHandleOmittedElem(int val);
XMLPUBFUN void htmlInitAutoClose (void);
XMLPUBFUN int htmlIsAutoClosed(xmlDoc *doc, xmlNode *elem);
XMLPUBFUN int htmlIsScriptAttribute(const xmlChar *name);
XMLPUBFUN htmlParserCtxt * htmlNewParserCtxt(void);
XMLPUBFUN htmlParserCtxt * htmlNewSAXParserCtxt(const htmlSAXHandler *sax, void *userData);
XMLPUBFUN htmlStatus htmlNodeStatus(xmlNode *, int) ;
XMLPUBFUN int htmlParseCharRef(htmlParserCtxt *ctxt);
XMLPUBFUN int htmlParseChunk (htmlParserCtxt *ctxt, const char *chunk, int size, int terminate);
XMLPUBFUN int htmlParseDocument(htmlParserCtxt *ctxt);
XMLPUBFUN void htmlParseElement(htmlParserCtxt *ctxt);
XMLPUBFUN const htmlEntityDesc * htmlParseEntityRef(htmlParserCtxt *ctxt, const xmlChar **str);
XMLPUBFUN xmlDoc * htmlReadDoc (const xmlChar *cur, const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDoc * htmlReadFd (int fd, const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDoc * htmlReadFile (const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDoc * htmlReadIO (xmlInputReadCallback ioread, xmlInputCloseCallback ioclose, void *ioctx, const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDoc * htmlReadMemory (const char *buffer, int size, const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDoc * htmlSAXParseDoc (const xmlChar *cur, const char *encoding, htmlSAXHandler *sax, void *userData);
XMLPUBFUN xmlDoc * htmlSAXParseFile(const char *filename, const char *encoding, htmlSAXHandler *sax, void *userData);
XMLPUBFUN const htmlElemDesc * htmlTagLookup (const xmlChar *tag);
XMLPUBFUN int htmlUTF8ToHtml (unsigned char *out, int *outlen, const unsigned char *in, int *inlen);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __HTML_PARSER_H__ */
