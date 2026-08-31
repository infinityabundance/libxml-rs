/**
 * @file
 *
 * XML Reader API for libxml-rs
 *
 * # UPSTREAM-PARITY
 * Enums, callback typedefs and `xmlTextReader` layout follow upstream
 * `xmlreader.h` (libxml2 2.15.x).
 */

#ifndef __XML_XMLREADER_H__
#define __XML_XMLREADER_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlIO.h>
#include <libxml/xmlerror.h>
#include <libxml/xmlschemas.h>
#include <libxml/relaxng.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlTextReader xmlTextReader;
typedef xmlTextReader *xmlTextReaderPtr;

/* Reader node types (upstream xmlReaderTypes) */
typedef enum {
    XML_READER_TYPE_NONE = 0,
    XML_READER_TYPE_ELEMENT = 1,
    XML_READER_TYPE_ATTRIBUTE = 2,
    XML_READER_TYPE_TEXT = 3,
    XML_READER_TYPE_CDATA = 4,
    XML_READER_TYPE_ENTITY_REFERENCE = 5,
    XML_READER_TYPE_ENTITY = 6,
    XML_READER_TYPE_PROCESSING_INSTRUCTION = 7,
    XML_READER_TYPE_COMMENT = 8,
    XML_READER_TYPE_DOCUMENT = 9,
    XML_READER_TYPE_DOCUMENT_TYPE = 10,
    XML_READER_TYPE_DOCUMENT_FRAGMENT = 11,
    XML_READER_TYPE_NOTATION = 12,
    XML_READER_TYPE_WHITESPACE = 13,
    XML_READER_TYPE_SIGNIFICANT_WHITESPACE = 14,
    XML_READER_TYPE_END_ELEMENT = 15,
    XML_READER_TYPE_END_ENTITY = 16,
    XML_READER_TYPE_XML_DECLARATION = 17
} xmlReaderTypes;

/* Reader modes (upstream xmlTextReaderMode) */
typedef enum {
    XML_TEXTREADER_MODE_INITIAL = 0,
    XML_TEXTREADER_MODE_INTERACTIVE = 1,
    XML_TEXTREADER_MODE_ERROR = 2,
    XML_TEXTREADER_MODE_EOF = 3,
    XML_TEXTREADER_MODE_CLOSED = 4,
    XML_TEXTREADER_MODE_READING = 5
} xmlTextReaderMode;

/* Parser properties (upstream xmlParserProperties) */
typedef enum {
    XML_PARSER_LOADDTD = 1,
    XML_PARSER_DEFAULTATTRS = 2,
    XML_PARSER_VALIDATE = 3,
    XML_PARSER_SUBST_ENTITIES = 4
} xmlParserProperties;

/* Error severity (upstream xmlParserSeverities) */
typedef enum {
    XML_PARSER_SEVERITY_VALIDITY_WARNING = 1,
    XML_PARSER_SEVERITY_VALIDITY_ERROR = 2,
    XML_PARSER_SEVERITY_WARNING = 3,
    XML_PARSER_SEVERITY_ERROR = 4
} xmlParserSeverities;

/* Locator / error callbacks (upstream xmlreader.h) */
typedef void *xmlTextReaderLocatorPtr;
typedef void (*xmlTextReaderErrorFunc)(void *arg,
				       const char *msg,
				       xmlParserSeverities severity,
				       xmlTextReaderLocatorPtr locator);

XMLPUBFUN int xmlTextReaderRead(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderNext(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstValue(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstString(xmlTextReaderPtr reader,
                                                  const xmlChar *str);
XMLPUBFUN int xmlTextReaderNodeType(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderDepth(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderIsEmptyElement(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderName(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderValue(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderLocalName(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderPrefix(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderHasAttributes(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderMoveToFirstAttribute(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderMoveToNextAttribute(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderMoveToElement(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderMoveToAttribute(xmlTextReaderPtr reader,
                                           const xmlChar *name);
XMLPUBFUN int xmlTextReaderMoveToAttributeNo(xmlTextReaderPtr reader,
                                             int no);
XMLPUBFUN int xmlTextReaderMoveToAttributeNs(xmlTextReaderPtr reader,
                                             const xmlChar *localName,
                                             const xmlChar *namespaceURI);
XMLPUBFUN xmlChar *xmlTextReaderGetAttribute(xmlTextReaderPtr reader,
                                             const xmlChar *name);
XMLPUBFUN xmlChar *xmlTextReaderGetAttributeNo(xmlTextReaderPtr reader,
                                               int no);
XMLPUBFUN xmlChar *xmlTextReaderGetAttributeNs(xmlTextReaderPtr reader,
                                               const xmlChar *localName,
                                               const xmlChar *namespaceURI);
XMLPUBFUN int xmlTextReaderAttributeCount(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderIsValid(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderGetParserLineNumber(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderGetParserColumnNumber(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderGetParserProp(xmlTextReaderPtr reader,
                                         int prop);
XMLPUBFUN int xmlTextReaderSetParserProp(xmlTextReaderPtr reader,
                                         int prop, int value);
XMLPUBFUN xmlChar *xmlTextReaderReadString(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderReadInnerXml(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderReadOuterXml(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderClose(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderReadAttributeValue(xmlTextReaderPtr reader);
XMLPUBFUN xmlDocPtr xmlTextReaderCurrentDoc(xmlTextReaderPtr reader);
XMLPUBFUN xmlNodePtr xmlTextReaderCurrentNode(xmlTextReaderPtr reader);
XMLPUBFUN xmlNodePtr xmlTextReaderExpand(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderStandalone(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderNormalization(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderBaseUri(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderXmlLang(xmlTextReaderPtr reader);
XMLPUBFUN xmlTextReaderPtr xmlReaderForDoc(const xmlChar *cur,
                                           const char *URL,
                                           const char *encoding,
                                           int options);
XMLPUBFUN xmlTextReaderPtr xmlReaderForFile(const char *filename,
                                            const char *encoding,
                                            int options);
XMLPUBFUN xmlTextReaderPtr xmlReaderForMemory(const char *buffer,
                                              int size,
                                              const char *URL,
                                              const char *encoding,
                                              int options);
XMLPUBFUN xmlTextReaderPtr xmlReaderForFd(int fd, const char *URL,
                                          const char *encoding, int options);
XMLPUBFUN xmlTextReaderPtr xmlReaderForIO(xmlInputReadCallback ioread,
                                          xmlInputCloseCallback ioclose,
                                          void *ioctx,
                                          const char *URL,
                                          const char *encoding,
                                          int options);
XMLPUBFUN xmlTextReaderPtr xmlReaderWalker(xmlDocPtr doc);
XMLPUBFUN xmlTextReaderPtr xmlNewTextReaderFilename(const char *URI);
XMLPUBFUN int xmlReaderNewDoc(xmlTextReaderPtr reader,
                              const xmlChar *cur,
                              const char *URL,
                              const char *encoding,
                              int options);
XMLPUBFUN int xmlReaderNewFile(xmlTextReaderPtr reader,
                               const char *filename,
                               const char *encoding,
                               int options);
XMLPUBFUN int xmlReaderNewMemory(xmlTextReaderPtr reader,
                                 const char *buffer,
                                 int size,
                                 const char *URL,
                                 const char *encoding,
                                 int options);
XMLPUBFUN int xmlReaderNewFd(xmlTextReaderPtr reader, int fd,
                             const char *URL, const char *encoding,
                             int options);
XMLPUBFUN int xmlReaderNewIO(xmlTextReaderPtr reader,
                             xmlInputReadCallback ioread,
                             xmlInputCloseCallback ioclose,
                             void *ioctx,
                             const char *URL,
                             const char *encoding,
                             int options);
XMLPUBFUN int xmlReaderNewWalker(xmlTextReaderPtr reader, xmlDocPtr doc);
XMLPUBFUN void xmlFreeTextReader(xmlTextReaderPtr reader);
XMLPUBFUN long xmlTextReaderByteConsumed(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstBaseUri(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstEncoding(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstLocalName(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstName(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstNamespaceUri(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstPrefix(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstXmlLang(xmlTextReaderPtr reader);
XMLPUBFUN const xmlChar *xmlTextReaderConstXmlVersion(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderQuoteChar(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderIsDefault(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderIsNamespaceDecl(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderMoveToAttributeNs(xmlTextReaderPtr reader,
                                             const xmlChar *localName,
                                             const xmlChar *namespaceURI);
XMLPUBFUN xmlNodePtr xmlTextReaderPreserve(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderPreservePattern(xmlTextReaderPtr reader,
                                           const xmlChar *pattern,
                                           const xmlChar **namespaces);
XMLPUBFUN void xmlTextReaderSetErrorHandler(xmlTextReaderPtr reader,
                                            xmlTextReaderErrorFunc f,
                                            void *arg);
XMLPUBFUN void xmlTextReaderGetErrorHandler(xmlTextReaderPtr reader,
                                            xmlTextReaderErrorFunc *f,
                                            void **arg);
XMLPUBFUN void xmlTextReaderSetStructuredErrorHandler(xmlTextReaderPtr reader,
                                                      xmlStructuredErrorFunc f,
                                                      void *arg);
XMLPUBFUN const xmlError *xmlTextReaderGetLastError(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderLocatorBaseURI(xmlTextReaderLocatorPtr locator);
XMLPUBFUN int xmlTextReaderLocatorLineNumber(xmlTextReaderLocatorPtr locator);
XMLPUBFUN xmlParserInputBufferPtr xmlTextReaderGetRemainder(xmlTextReaderPtr reader);
XMLPUBFUN void xmlTextReaderSetMaxAmplification(xmlTextReaderPtr reader, unsigned int maxAmpl);
XMLPUBFUN int xmlTextReaderSchemaValidate(xmlTextReaderPtr reader, const char *xsd);
XMLPUBFUN int xmlTextReaderSchemaValidateCtxt(xmlTextReaderPtr reader,
                                               xmlSchemaValidCtxtPtr ctxt,
                                               int options);
XMLPUBFUN int xmlTextReaderSetSchema(xmlTextReaderPtr reader, xmlSchemaPtr schema);
XMLPUBFUN int xmlTextReaderRelaxNGValidate(xmlTextReaderPtr reader, const char *rng);
XMLPUBFUN int xmlTextReaderRelaxNGValidateCtxt(xmlTextReaderPtr reader,
                                                xmlRelaxNGValidCtxtPtr ctxt,
                                                int options);
XMLPUBFUN int xmlTextReaderRelaxNGSetSchema(xmlTextReaderPtr reader,
                                            xmlRelaxNGPtr schema);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN xmlTextReader * xmlNewTextReader (xmlParserInputBuffer *input, const char *URI);
XMLPUBFUN int xmlTextReaderHasValue(xmlTextReader *reader);
XMLPUBFUN xmlChar * xmlTextReaderLookupNamespace(xmlTextReader *reader, const xmlChar *prefix);
XMLPUBFUN xmlChar * xmlTextReaderNamespaceUri(xmlTextReader *reader);
XMLPUBFUN int xmlTextReaderNextSibling (xmlTextReader *reader);
XMLPUBFUN int xmlTextReaderReadState (xmlTextReader *reader);
XMLPUBFUN void xmlTextReaderSetResourceLoader(xmlTextReader *reader, xmlResourceLoader loader, void *data);
XMLPUBFUN int xmlTextReaderSetup(xmlTextReader *reader, xmlParserInputBuffer *input, const char *URL, const char *encoding, int options);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_XMLREADER_H__ */
