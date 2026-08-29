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
XMLPUBFUN xmlChar *xmlTextReaderConstValue(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderConstString(xmlTextReaderPtr reader,
                                            const xmlChar *str);
XMLPUBFUN int xmlTextReaderNodeType(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderDepth(xmlTextReaderPtr reader);
XMLPUBFUN int xmlTextReaderIsEmptyElement(xmlTextReaderPtr reader);
XMLPUBFUN xmlChar *xmlTextReaderName(xmlTextReaderPtr reader);
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
XMLPUBFUN int xmlTextReaderGetAttributeIndex(xmlTextReaderPtr reader,
                                             const xmlChar *name);
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
XMLPUBFUN xmlTextReaderPtr xmlReaderNewDoc(xmlTextReaderPtr reader,
                                           const xmlChar *cur,
                                           const char *URL,
                                           const char *encoding,
                                           int options);
XMLPUBFUN xmlTextReaderPtr xmlReaderNewFile(xmlTextReaderPtr reader,
                                            const char *filename,
                                            const char *encoding,
                                            int options);
XMLPUBFUN xmlTextReaderPtr xmlReaderNewMemory(xmlTextReaderPtr reader,
                                              const char *buffer,
                                              int size,
                                              const char *URL,
                                              const char *encoding,
                                              int options);
XMLPUBFUN void xmlFreeTextReader(xmlTextReaderPtr reader);

#ifdef __cplusplus
}
#endif

#endif /* __XML_XMLREADER_H__ */
