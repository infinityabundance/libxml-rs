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
#include <libxml/encoding.h>
#include <libxml/SAX2.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Parser options */
#define XML_PARSE_RECOVER 1
#define XML_PARSE_NOENT 2
#define XML_PARSE_DTDLOAD 4
#define XML_PARSE_DTDATTR 8
#define XML_PARSE_DTDVALID 16
#define XML_PARSE_NOERROR 32
#define XML_PARSE_NOWARNING 64
#define XML_PARSE_PEDANTIC 128
#define XML_PARSE_NOBLANKS 256
#define XML_PARSE_SAX1 512
#define XML_PARSE_XINCLUDE 1024
#define XML_PARSE_NONET 2048
#define XML_PARSE_NODICT 4096
#define XML_PARSE_NSCLEAN 8192
#define XML_PARSE_NOCDATA 16384
#define XML_PARSE_NOXINCNODE 32768
#define XML_PARSE_COMPACT 65536
#define XML_PARSE_OLD10 131072
#define XML_PARSE_NOBASEFIX 262144
#define XML_PARSE_HUGE 524288
#define XML_PARSE_OLDSAX 1048576
#define XML_PARSE_IGNORE_ENC 2097152
#define XML_PARSE_BIG_LINES 4194304

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
typedef struct _xmlParserNodeInfo xmlParserNodeInfo;
typedef xmlParserNodeInfo *xmlParserNodeInfoPtr;
struct _xmlValidCtxt;
typedef struct _xmlValidCtxt xmlValidCtxt;
typedef xmlValidCtxt *xmlValidCtxtPtr;

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

/* Parser input structure */
typedef struct _xmlParserInput xmlParserInput;
typedef xmlParserInput *xmlParserInputPtr;
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

/* Parser input buffer */
typedef struct _xmlParserInputBuffer xmlParserInputBuffer;
typedef xmlParserInputBuffer *xmlParserInputBufferPtr;
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

/* Output buffer */
typedef struct _xmlOutputBuffer xmlOutputBuffer;
typedef xmlOutputBuffer *xmlOutputBufferPtr;
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
    int node_seq;
    int errNo;
    int hasExternalSubset;
    int hasPErefs;
    int external;
    int valid;
    int validate;
    xmlValidCtxtPtr vctxt;
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
    int attallocs;
    xmlNodePtr *pushTab;
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
    int nbentities;
    int sizeentities;
    xmlParserNodeInfoPtr nodeInfo;
    int nodeInfoNr;
    int nodeInfoMax;
    xmlParserNodeInfo *nodeInfoTab;
    int input_id;
    int sizeentcopy;
    int endCheckState;
    int nbErrors;
    int nbWarnings;
    int maxAmpl;
    int nsdb;
    int attrHashMax;
    xmlHashTablePtr attrHash;
    xmlGenericErrorFunc errorHandler;
    void *errorCtxt;
    xmlResourceLoader resourceLoader;
    void *resourceCtxt;
    xmlCharEncodingInputFunc convImpl;
    void *convCtxt;
};



struct _xmlParserNodeInfo {
    xmlNodePtr node;
    unsigned long begin_pos;
    unsigned long begin_line;
    unsigned long end_pos;
    unsigned long end_line;
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

#ifdef __cplusplus
}
#endif

#endif /* __XML_PARSER_H__ */
