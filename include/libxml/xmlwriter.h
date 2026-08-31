/*
 * xmlwriter.h — XML Writer API for libxml-rs.
 *
 * Declarations mirror upstream libxml2 xmlwriter.h (2.15.3) with the same
 * signatures; the implementation is the native-Rust xmlTextWriter family in
 * src/xml/writer/mod.rs.
 */

#ifndef __XML_XMLWRITER_H__
#define __XML_XMLWRITER_H__

#include <stdarg.h>
#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlIO.h>

/* printf-style attribute for the Format functions (upstream: xmlexports.h). */
#ifndef LIBXML_ATTR_FORMAT
#if defined(__GNUC__) || defined(__clang__)
#define LIBXML_ATTR_FORMAT(fmt, args) __attribute__((format(printf, fmt, args)))
#else
#define LIBXML_ATTR_FORMAT(fmt, args)
#endif
#endif

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlTextWriter xmlTextWriter;
typedef xmlTextWriter *xmlTextWriterPtr;

/*
 * Constructors
 */
XMLPUBFUN xmlTextWriterPtr
		xmlNewTextWriter	(xmlOutputBufferPtr out);
XMLPUBFUN xmlTextWriterPtr
		xmlNewTextWriterFilename(const char *uri, int compression);
XMLPUBFUN xmlTextWriterPtr
		xmlNewTextWriterMemory	(xmlBufferPtr buf, int compression);
XMLPUBFUN xmlTextWriterPtr
		xmlNewTextWriterDoc	(xmlDocPtr *doc, int compression);
XMLPUBFUN xmlTextWriterPtr
		xmlNewTextWriterTree	(xmlDocPtr doc, xmlNodePtr node,
					 int compression);
XMLPUBFUN void
		xmlFreeTextWriter	(xmlTextWriterPtr writer);

/*
 * Writer functions
 */
XMLPUBFUN int
		xmlTextWriterStartDocument(xmlTextWriterPtr writer,
					 const char *version,
					 const char *encoding,
					 const char *standalone);
XMLPUBFUN int
		xmlTextWriterEndDocument	(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterStartElement	(xmlTextWriterPtr writer,
					 const xmlChar *name);
XMLPUBFUN int
		xmlTextWriterStartElementNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI);
XMLPUBFUN int
		xmlTextWriterEndElement		(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterFullEndElement	(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterWriteElement	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteElementNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteFormatElement	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN int
		xmlTextWriterWriteVFormatElement	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN int
		xmlTextWriterWriteFormatElementNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI,
					 const char *format, ...) LIBXML_ATTR_FORMAT(5,6);
XMLPUBFUN int
		xmlTextWriterWriteVFormatElementNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(5,0);
XMLPUBFUN int
		xmlTextWriterStartAttribute	(xmlTextWriterPtr writer,
					 const xmlChar *name);
XMLPUBFUN int
		xmlTextWriterStartAttributeNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI);
XMLPUBFUN int
		xmlTextWriterEndAttribute	(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterWriteAttribute	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteAttributeNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteFormatAttribute	(xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN int
		xmlTextWriterWriteFormatAttributeNS	(xmlTextWriterPtr writer,
						 const xmlChar *prefix,
						 const xmlChar *name,
						 const xmlChar *namespaceURI,
						 const char *format, ...) LIBXML_ATTR_FORMAT(5,6);
XMLPUBFUN int
		xmlTextWriterWriteVFormatAttribute	(xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN int
		xmlTextWriterWriteVFormatAttributeNS	(xmlTextWriterPtr writer,
						 const xmlChar *prefix,
						 const xmlChar *name,
						 const xmlChar *namespaceURI,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(5,0);
XMLPUBFUN int
		xmlTextWriterWriteString		(xmlTextWriterPtr writer,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteBase64		(xmlTextWriterPtr writer,
					 const char *data,
					 int start,
					 int len);
XMLPUBFUN int
		xmlTextWriterWriteBinHex		(xmlTextWriterPtr writer,
					 const char *data,
					 int start,
					 int len);
XMLPUBFUN int
		xmlTextWriterWriteRaw		(xmlTextWriterPtr writer,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteRawLen		(xmlTextWriterPtr writer,
					 const xmlChar *content,
					 int len);
XMLPUBFUN int
		xmlTextWriterWriteFormatRaw	(xmlTextWriterPtr writer,
					 const char *format, ...) LIBXML_ATTR_FORMAT(2,3);
XMLPUBFUN int
		xmlTextWriterWriteVFormatRaw	(xmlTextWriterPtr writer,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(2,0);
XMLPUBFUN int
		xmlTextWriterWriteFormatString	(xmlTextWriterPtr writer,
					 const char *format, ...) LIBXML_ATTR_FORMAT(2,3);
XMLPUBFUN int
		xmlTextWriterWriteVFormatString	(xmlTextWriterPtr writer,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(2,0);
XMLPUBFUN int
		xmlTextWriterWriteComment	(xmlTextWriterPtr writer,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteFormatComment	(xmlTextWriterPtr writer,
					 const char *format, ...) LIBXML_ATTR_FORMAT(2,3);
XMLPUBFUN int
		xmlTextWriterWriteVFormatComment	(xmlTextWriterPtr writer,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(2,0);
XMLPUBFUN int
		xmlTextWriterWritePI		(xmlTextWriterPtr writer,
					 const xmlChar *target,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteFormatPI	(xmlTextWriterPtr writer,
					 const xmlChar *target,
					 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN int
		xmlTextWriterWriteVFormatPI	(xmlTextWriterPtr writer,
					 const xmlChar *target,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN int
		xmlTextWriterWriteCDATA		(xmlTextWriterPtr writer,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteFormatCDATA	(xmlTextWriterPtr writer,
					 const char *format, ...) LIBXML_ATTR_FORMAT(2,3);
XMLPUBFUN int
		xmlTextWriterWriteVFormatCDATA	(xmlTextWriterPtr writer,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(2,0);
XMLPUBFUN int
		xmlTextWriterStartCDATA		(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterEndCDATA		(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterStartComment	(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterEndComment		(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterStartPI		(xmlTextWriterPtr writer,
					 const xmlChar *target);
XMLPUBFUN int
		xmlTextWriterEndPI		(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterStartDTD		(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid);
XMLPUBFUN int
		xmlTextWriterEndDTD		(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterStartDTDElement	(xmlTextWriterPtr writer,
					 const xmlChar *name);
XMLPUBFUN int
		xmlTextWriterEndDTDElement	(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterWriteDTDElement	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteFormatDTDElement (xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN int
		xmlTextWriterWriteVFormatDTDElement (xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN int
		xmlTextWriterStartDTDAttlist	(xmlTextWriterPtr writer,
					 const xmlChar *name);
XMLPUBFUN int
		xmlTextWriterEndDTDAttlist	(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterWriteDTDAttlist	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteFormatDTDAttlist (xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN int
		xmlTextWriterWriteVFormatDTDAttlist (xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN int
		xmlTextWriterStartDTDEntity	(xmlTextWriterPtr writer,
					 int pe,
					 const xmlChar *name);
XMLPUBFUN int
		xmlTextWriterEndDTDEntity	(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterWriteDTDEntity	(xmlTextWriterPtr writer,
					 int pe,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid,
					 const xmlChar *ndataid,
					 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteDTDInternalEntity (xmlTextWriterPtr writer,
						 int pe,
						 const xmlChar *name,
						 const xmlChar *content);
XMLPUBFUN int
		xmlTextWriterWriteDTDExternalEntity (xmlTextWriterPtr writer,
						 int pe,
						 const xmlChar *name,
						 const xmlChar *pubid,
						 const xmlChar *sysid,
						 const xmlChar *ndataid);
XMLPUBFUN int
		xmlTextWriterWriteDTDExternalEntityContents (xmlTextWriterPtr writer,
						 const xmlChar *pubid,
						 const xmlChar *sysid,
						 const xmlChar *ndataid);
XMLPUBFUN int
		xmlTextWriterWriteDTDNotation	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid);
XMLPUBFUN int
		xmlTextWriterWriteFormatDTDInternalEntity (xmlTextWriterPtr writer,
						 int pe,
						 const xmlChar *name,
						 const char *format, ...) LIBXML_ATTR_FORMAT(4,5);
XMLPUBFUN int
		xmlTextWriterWriteVFormatDTDInternalEntity (xmlTextWriterPtr writer,
						 int pe,
						 const xmlChar *name,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(4,0);
XMLPUBFUN int
		xmlTextWriterWriteFormatDTD	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid,
					 const char *format, ...) LIBXML_ATTR_FORMAT(5,6);
XMLPUBFUN int
		xmlTextWriterWriteVFormatDTD	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(5,0);
XMLPUBFUN int
		xmlTextWriterWriteDTD		(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid,
					 const xmlChar *subset);
XMLPUBFUN int
		xmlTextWriterSetIndent		(xmlTextWriterPtr writer,
					 int indent);
XMLPUBFUN int
		xmlTextWriterSetIndentString	(xmlTextWriterPtr writer,
					 const xmlChar *str);
XMLPUBFUN int
		xmlTextWriterSetQuoteChar	(xmlTextWriterPtr writer,
					 xmlChar quotechar);
XMLPUBFUN int
		xmlTextWriterFlush		(xmlTextWriterPtr writer);
XMLPUBFUN int
		xmlTextWriterClose		(xmlTextWriterPtr writer);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN xmlTextWriter * xmlNewTextWriterPushParser(xmlParserCtxt *ctxt, int compression);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_XMLWRITER_H__ */
