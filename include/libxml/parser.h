/**
 * @file
 *
 * XML parser API for libxml-rs
 */

#ifndef __XML_PARSER_H__
#define __XML_PARSER_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>
#include <libxml/xmlmemory.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <libxml/xmlIO.h>
#include <libxml/dict.h>
#include <libxml/hash.h>
#include <libxml/valid.h>
#include <libxml/encoding.h>
#include <libxml/SAX2.h>

#ifdef __cplusplus
extern "C" {
#endif

























































































































































/* Parser options */

/* Parser mode */
typedef enum {
    XML_PARSE_UNKNOWN = 0,
    XML_PARSE_DOM = 1,
    XML_PARSE_SAX = 2,
    XML_PARSE_PUSH_DOM = 3,
    XML_PARSE_PUSH_SAX = 4,
    XML_PARSE_READER = 5
} xmlParserMode;

/* Forward declarations */
struct _xmlValidCtxt;

/* Parser input states */
typedef enum {
    XML_PARSER_EOF = -1,
    XML_PARSER_START = 0,
    XML_PARSER_MISC = 1,
    XML_PARSER_DTD = 2,
    XML_PARSER_PROLOG = 3,
    XML_PARSER_CONTENT = 4,
    XML_PARSER_CDATA_SECTION = 5,
    XML_PARSER_ENTITY_REF = 6,
    XML_PARSER_ENTITY_VALUE = 7,
    XML_PARSER_ATTRIBUTE_VALUE = 8,
    XML_PARSER_SYSTEM_LITERAL = 9,
    XML_PARSER_EPILOG = 10,
    XML_PARSER_IGNORE = 11,
    XML_PARSER_PUBLIC_LITERAL = 12
} xmlParserInputState;

/* Parser input structure (fwd typedef in SAX2.h; struct body here) */
struct _xmlParserInput {
    xmlParserInputBufferPtr buf;
    const char *filename;
    const char *directory;
    const xmlChar *base;
    const xmlChar *cur;
    const xmlChar *end;
    int length;
    int line;
    int col;
    unsigned long consumed;
    xmlFreeFunc free;
    const char *encoding;
    const xmlChar *version;
    int flags;
    int id;
    unsigned long parentConsumed;
    xmlEntityPtr entity;
};

/* Parser input buffer (typedef in tree.h; struct body here) */
struct _xmlParserInputBuffer {
    void *context;
    xmlInputReadCallback readcallback;
    xmlInputCloseCallback closecallback;
    xmlCharEncodingHandlerPtr encoder;
    xmlBufferPtr buffer;
    xmlBufferPtr raw;
    int compressed;
    int error;
    unsigned long rawconsumed;
};

/* Output buffer (typedef in tree.h; struct body here) */
struct _xmlOutputBuffer {
    void *context;
    xmlOutputWriteCallback writecallback;
    xmlOutputCloseCallback closecallback;
    xmlCharEncodingHandlerPtr encoder;
    xmlBufferPtr buffer;
    xmlBufferPtr conv;
    int written;
    int error;
};

/* Resource types passed to xmlResourceLoader (upstream parser.h) */
typedef enum {
    XML_RESOURCE_UNKNOWN = 0,
    XML_RESOURCE_MAIN_DOCUMENT,
    XML_RESOURCE_DTD,
    XML_RESOURCE_GENERAL_ENTITY,
    XML_RESOURCE_PARAMETER_ENTITY,
    XML_RESOURCE_XINCLUDE,
    XML_RESOURCE_XINCLUDE_TEXT
} xmlResourceType;

/* Flags for parser input (upstream parser.h) */
typedef enum {
    XML_INPUT_BUF_STATIC = (1 << 1),
    XML_INPUT_BUF_ZERO_TERMINATED = (1 << 2),
    XML_INPUT_UNZIP = (1 << 3),
    XML_INPUT_NETWORK = (1 << 4),
    XML_INPUT_USE_SYS_CATALOG = (1 << 5)
} xmlParserInputFlags;

/* Opaque structs referenced by _xmlParserCtxt (upstream parser.h) */
typedef struct _xmlStartTag xmlStartTag;
typedef struct _xmlParserNsData xmlParserNsData;
typedef struct _xmlAttrHashBucket xmlAttrHashBucket;

/* Custom resource loader callback (upstream parser.h) */
typedef xmlParserErrors
(*xmlResourceLoader)(void *ctxt, const char *url, const char *publicId,
                     xmlResourceType type, xmlParserInputFlags flags,
                     xmlParserInput **out);

/* Parser context */
typedef struct _xmlParserCtxt xmlParserCtxt;
typedef xmlParserCtxt *xmlParserCtxtPtr;
struct _xmlParserCtxt {
    xmlSAXHandlerPtr sax;
    void *userData;
    xmlDocPtr myDoc;
    int wellFormed;
    int replaceEntities;
    const xmlChar *version;
    const xmlChar *encoding;
    int standalone;
    int html;
    xmlParserInputPtr input;
    int inputNr;
    int inputMax;
    xmlParserInputPtr *inputTab;
    xmlNodePtr node;
    int nodeNr;
    int nodeMax;
    xmlNodePtr *nodeTab;
    int record_info;
    xmlParserNodeInfoSeq node_seq;
    int errNo;
    int hasExternalSubset;
    int hasPErefs;
    int external;
    int valid;
    int validate;
    xmlValidCtxt vctxt;
    int instate;
    int token;
    char *directory;
    xmlChar *name;
    int nameNr;
    int nameMax;
    xmlChar **nameTab;
    long nbChars;
    long checkIndex;
    int keepBlanks;
    int disableSAX;
    int inSubset;
    const xmlChar *intSubName;
    xmlChar *extSubURI;
    xmlChar *extSubSystem;
    int *space;
    int spaceNr;
    int spaceMax;
    int *spaceTab;
    int depth;
    xmlEntityPtr entity;
    int charset;
    int nodelen;
    int nodemem;
    int pedantic;
    void *_private;
    int loadsubset;
    int linenumbers;
    void *catalogs;
    int recovery;
    int progressive;
    xmlDictPtr dict;
    const xmlChar **atts;
    int maxatts;
    int docdict;
    const xmlChar *str_xml;
    const xmlChar *str_xmlns;
    const xmlChar *str_xml_ns;
    int sax2;
    int nsNr;
    int nsMax;
    xmlNsPtr *nsTab;
    unsigned int *attallocs;
    xmlStartTag *pushTab;
    xmlHashTablePtr attsDefault;
    xmlHashTablePtr attsSpecial;
    int nsWellFormed;
    int options;
    int dictNames;
    int freeElemsNr;
    xmlNodePtr *freeElems;
    int freeAttrsNr;
    xmlAttrPtr *freeAttrs;
    xmlError lastError;
    int parseMode;
    unsigned long nbentities;
    unsigned long sizeentities;
    xmlParserNodeInfoPtr nodeInfo;
    int nodeInfoNr;
    int nodeInfoMax;
    xmlParserNodeInfo *nodeInfoTab;
    int input_id;
    unsigned long sizeentcopy;
    int endCheckState;
    unsigned short nbErrors;
    unsigned short nbWarnings;
    unsigned int maxAmpl;
    xmlParserNsData *nsdb;
    unsigned int attrHashMax;
    xmlAttrHashBucket *attrHash;
    xmlStructuredErrorFunc errorHandler;
    void *errorCtxt;
    xmlResourceLoader resourceLoader;
    void *resourceCtxt;
    xmlCharEncConvImpl convImpl;
    void *convCtxt;
};


/* Init and cleanup */
XMLPUBFUN void xmlInitParser(void);
XMLPUBFUN void xmlCleanupParser(void);
XMLPUBFUN int xmlIsInitialized(void);

/* Reading APIs */
XMLPUBFUN xmlDocPtr xmlReadDoc(const xmlChar *cur, const char *URL,
                                const char *encoding, int options);
XMLPUBFUN xmlDocPtr xmlReadFile(const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDocPtr xmlReadMemory(const char *buffer, int size,
                                   const char *URL, const char *encoding, int options);
XMLPUBFUN xmlDocPtr xmlReadFd(int fd, const char *URL,
                               const char *encoding, int options);
XMLPUBFUN xmlDocPtr xmlReadIO(xmlInputReadCallback ioread,
                               xmlInputCloseCallback ioclose,
                               void *ioctx, const char *URL,
                               const char *encoding, int options);

/* Parse APIs */
XMLPUBFUN xmlDocPtr xmlParseDoc(const xmlChar *cur);
XMLPUBFUN xmlDocPtr xmlParseFile(const char *filename);
XMLPUBFUN xmlDocPtr xmlParseMemory(const char *buffer, int size);

/* SAX APIs */
XMLPUBFUN xmlDocPtr xmlSAXParseDoc(xmlSAXHandlerPtr sax, const xmlChar *cur, int recovery);
XMLPUBFUN xmlDocPtr xmlSAXParseFile(xmlSAXHandlerPtr sax, const char *filename, int recovery);
XMLPUBFUN xmlDocPtr xmlSAXParseMemory(xmlSAXHandlerPtr sax,
                                       const char *buffer, int size, int recovery);
XMLPUBFUN int xmlSAXUserParseFile(xmlSAXHandlerPtr sax, void *user_data,
                                   const char *filename);
XMLPUBFUN int xmlSAXUserParseMemory(xmlSAXHandlerPtr sax, void *user_data,
                                     const char *buffer, int size);

/* Context APIs */
XMLPUBFUN xmlParserCtxtPtr xmlCreateFileParserCtxt(const char *filename);
XMLPUBFUN xmlParserCtxtPtr xmlCreateDocParserCtxt(const xmlChar *cur);
XMLPUBFUN int xmlParseDocument(xmlParserCtxtPtr ctxt);
XMLPUBFUN void xmlFreeParserCtxt(xmlParserCtxtPtr ctxt);
XMLPUBFUN int xmlCtxtUseOptions(xmlParserCtxtPtr ctxt, int options);
XMLPUBFUN int xmlParseChunk(xmlParserCtxtPtr ctxt, const char *chunk,
                             int size, int terminate);

/* Input buffer APIs */
XMLPUBFUN xmlParserInputBufferPtr xmlParserInputBufferCreateMem(
    const char *buffer, int size, int enc);
XMLPUBFUN xmlParserInputBufferPtr xmlParserInputBufferCreateFilename(
    const char *URI, int enc);
XMLPUBFUN xmlParserInputBufferPtr xmlParserInputBufferCreateIO(
    xmlInputReadCallback ioread, xmlInputCloseCallback ioclose,
    void *ioctx, int enc);
XMLPUBFUN void xmlFreeParserInputBuffer(xmlParserInputBufferPtr buf);
XMLPUBFUN xmlParserInputPtr xmlNewInputFromFile(xmlParserCtxtPtr ctxt,
                                                 const char *filename);
XMLPUBFUN void xmlFreeInputStream(xmlParserInputPtr input);

/* Output buffer APIs */
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateFilename(
    const char *URI, xmlCharEncodingHandlerPtr encoder, int compression);
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateFd(
    int fd, xmlCharEncodingHandlerPtr encoder);
XMLPUBFUN xmlOutputBufferPtr xmlOutputBufferCreateIO(
    xmlOutputWriteCallback iowrite, xmlOutputCloseCallback ioclose,
    void *ioctx, xmlCharEncodingHandlerPtr encoder);
XMLPUBFUN int xmlOutputBufferClose(xmlOutputBufferPtr out);
XMLPUBFUN int xmlOutputBufferFlush(xmlOutputBufferPtr out);
XMLPUBFUN int xmlOutputBufferWrite(xmlOutputBufferPtr out, int len, const char *data);
XMLPUBFUN int xmlOutputBufferWriteString(xmlOutputBufferPtr out, const char *str);















































































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlSAXHandlerV1 xmlSAXHandlerV1;
typedef xmlSAXHandlerV1 *xmlSAXHandlerV1Ptr;
/**
 * SAX handler, version 1.
 *
 * @deprecated Use version 2 handlers.
 */

typedef enum{
    XML_PARSE_RECOVER = 1<<0,
    XML_PARSE_NOENT = 1<<1,
    XML_PARSE_DTDLOAD = 1<<2,
    XML_PARSE_DTDATTR = 1<<3,
    XML_PARSE_DTDVALID = 1<<4,
    XML_PARSE_NOERROR = 1<<5,
    XML_PARSE_NOWARNING = 1<<6,
    XML_PARSE_PEDANTIC = 1<<7,
    XML_PARSE_NOBLANKS = 1<<8,
    XML_PARSE_SAX1 = 1<<9,
    XML_PARSE_XINCLUDE = 1<<10,
    XML_PARSE_NONET = 1<<11,
    XML_PARSE_NODICT = 1<<12,
    XML_PARSE_NSCLEAN = 1<<13,
    XML_PARSE_NOCDATA = 1<<14,
    XML_PARSE_NOXINCNODE = 1<<15,
    XML_PARSE_COMPACT = 1<<16,
    XML_PARSE_OLD10 = 1<<17,
    XML_PARSE_NOBASEFIX = 1<<18,
    XML_PARSE_HUGE = 1<<19,
    XML_PARSE_OLDSAX = 1<<20,
    XML_PARSE_IGNORE_ENC = 1<<21,
    XML_PARSE_BIG_LINES = 1<<22,
    XML_PARSE_NO_XXE = 1<<23,
    XML_PARSE_UNZIP = 1<<24,
    XML_PARSE_NO_SYS_CATALOG = 1<<25,
    XML_PARSE_CATALOG_PI = 1<<26,
    XML_PARSE_SKIP_IDS = 1<<27
} xmlParserOption;

typedef enum{
    XML_STATUS_NOT_WELL_FORMED          = (1 << 0),
    XML_STATUS_NOT_NS_WELL_FORMED       = (1 << 1),
    XML_STATUS_DTD_VALIDATION_FAILED    = (1 << 2),
    XML_STATUS_CATASTROPHIC_ERROR       = (1 << 3)
} xmlParserStatus;

typedef enum{
    XML_WITH_THREAD = 1,
    XML_WITH_TREE = 2,
    XML_WITH_OUTPUT = 3,
    XML_WITH_PUSH = 4,
    XML_WITH_READER = 5,
    XML_WITH_PATTERN = 6,
    XML_WITH_WRITER = 7,
    XML_WITH_SAX1 = 8,
    XML_WITH_FTP = 9,
    XML_WITH_HTTP = 10,
    XML_WITH_VALID = 11,
    XML_WITH_HTML = 12,
    XML_WITH_LEGACY = 13,
    XML_WITH_C14N = 14,
    XML_WITH_CATALOG = 15,
    XML_WITH_XPATH = 16,
    XML_WITH_XPTR = 17,
    XML_WITH_XINCLUDE = 18,
    XML_WITH_ICONV = 19,
    XML_WITH_ISO8859X = 20,
    XML_WITH_UNICODE = 21,
    XML_WITH_REGEXP = 22,
    XML_WITH_AUTOMATA = 23,
    XML_WITH_EXPR = 24,
    XML_WITH_SCHEMAS = 25,
    XML_WITH_SCHEMATRON = 26,
    XML_WITH_MODULES = 27,
    XML_WITH_DEBUG = 28,
    XML_WITH_DEBUG_MEM = 29,
    XML_WITH_DEBUG_RUN = 30,
    XML_WITH_ZLIB = 31,
    XML_WITH_ICU = 32,
    XML_WITH_LZMA = 33,
    XML_WITH_RELAXNG = 34,
    XML_WITH_NONE = 99999 /* just to be sure of allocation size */
} xmlFeature;

struct _xmlSAXHandlerV1 {
    internalSubsetSAXFunc internalSubset;
    isStandaloneSAXFunc isStandalone;
    hasInternalSubsetSAXFunc hasInternalSubset;
    hasExternalSubsetSAXFunc hasExternalSubset;
    resolveEntitySAXFunc resolveEntity;
    getEntitySAXFunc getEntity;
    entityDeclSAXFunc entityDecl;
    notationDeclSAXFunc notationDecl;
    attributeDeclSAXFunc attributeDecl;
    elementDeclSAXFunc elementDecl;
    unparsedEntityDeclSAXFunc unparsedEntityDecl;
    setDocumentLocatorSAXFunc setDocumentLocator;
    startDocumentSAXFunc startDocument;
    endDocumentSAXFunc endDocument;
    startElementSAXFunc startElement;
    endElementSAXFunc endElement;
    referenceSAXFunc reference;
    charactersSAXFunc characters;
    ignorableWhitespaceSAXFunc ignorableWhitespace;
    processingInstructionSAXFunc processingInstruction;
    commentSAXFunc comment;
    warningSAXFunc warning;
    errorSAXFunc error;
    fatalErrorSAXFunc fatalError; /* unused error() get all the errors */
    getParameterEntitySAXFunc getParameterEntity;
    cdataBlockSAXFunc cdataBlock;
    externalSubsetSAXFunc externalSubset;
    unsigned int initialized;
};

/* [11.1-G] end: extracted definitions */
#ifdef __cplusplus
}
#endif

#endif /* __XML_PARSER_H__ */
