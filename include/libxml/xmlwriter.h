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
XMLPUBFUN xmlChar *
		xmlTextWriterStartDocument(xmlTextWriterPtr writer,
					 const char *version,
					 const char *encoding,
					 const char *standalone);
XMLPUBFUN xmlChar *
		xmlTextWriterEndDocument	(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterStartElement	(xmlTextWriterPtr writer,
					 const xmlChar *name);
XMLPUBFUN xmlChar *
		xmlTextWriterStartElementNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI);
XMLPUBFUN xmlChar *
		xmlTextWriterEndElement		(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterFullEndElement	(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteElement	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteElementNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatElement	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatElement	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatElementNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI,
					 const char *format, ...) LIBXML_ATTR_FORMAT(5,6);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatElementNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(5,0);
XMLPUBFUN xmlChar *
		xmlTextWriterStartAttribute	(xmlTextWriterPtr writer,
					 const xmlChar *name);
XMLPUBFUN xmlChar *
		xmlTextWriterStartAttributeNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI);
XMLPUBFUN xmlChar *
		xmlTextWriterEndAttribute	(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteAttribute	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteAttributeNS	(xmlTextWriterPtr writer,
					 const xmlChar *prefix,
					 const xmlChar *name,
					 const xmlChar *namespaceURI,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatAttribute	(xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatAttributeNS	(xmlTextWriterPtr writer,
						 const xmlChar *prefix,
						 const xmlChar *name,
						 const xmlChar *namespaceURI,
						 const char *format, ...) LIBXML_ATTR_FORMAT(5,6);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatAttribute	(xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatAttributeNS	(xmlTextWriterPtr writer,
						 const xmlChar *prefix,
						 const xmlChar *name,
						 const xmlChar *namespaceURI,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(5,0);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteString		(xmlTextWriterPtr writer,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteBase64		(xmlTextWriterPtr writer,
					 const char *data,
					 int start,
					 int len);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteBinHex		(xmlTextWriterPtr writer,
					 const char *data,
					 int start,
					 int len);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteRaw		(xmlTextWriterPtr writer,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteRawLen		(xmlTextWriterPtr writer,
					 const xmlChar *content,
					 int len);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatRaw	(xmlTextWriterPtr writer,
					 const char *format, ...) LIBXML_ATTR_FORMAT(2,3);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatRaw	(xmlTextWriterPtr writer,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(2,0);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatString	(xmlTextWriterPtr writer,
					 const char *format, ...) LIBXML_ATTR_FORMAT(2,3);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatString	(xmlTextWriterPtr writer,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(2,0);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteComment	(xmlTextWriterPtr writer,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatComment	(xmlTextWriterPtr writer,
					 const char *format, ...) LIBXML_ATTR_FORMAT(2,3);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatComment	(xmlTextWriterPtr writer,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(2,0);
XMLPUBFUN xmlChar *
		xmlTextWriterWritePI		(xmlTextWriterPtr writer,
					 const xmlChar *target,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatPI	(xmlTextWriterPtr writer,
					 const xmlChar *target,
					 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatPI	(xmlTextWriterPtr writer,
					 const xmlChar *target,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteCDATA		(xmlTextWriterPtr writer,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatCDATA	(xmlTextWriterPtr writer,
					 const char *format, ...) LIBXML_ATTR_FORMAT(2,3);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatCDATA	(xmlTextWriterPtr writer,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(2,0);
XMLPUBFUN xmlChar *
		xmlTextWriterStartCDATA		(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterEndCDATA		(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterStartComment	(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterEndComment		(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterStartPI		(xmlTextWriterPtr writer,
					 const xmlChar *target);
XMLPUBFUN xmlChar *
		xmlTextWriterEndPI		(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterStartDTD		(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid);
XMLPUBFUN xmlChar *
		xmlTextWriterEndDTD		(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterStartDTDElement	(xmlTextWriterPtr writer,
					 const xmlChar *name);
XMLPUBFUN xmlChar *
		xmlTextWriterEndDTDElement	(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteDTDElement	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatDTDElement (xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatDTDElement (xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN xmlChar *
		xmlTextWriterStartDTDAttlist	(xmlTextWriterPtr writer,
					 const xmlChar *name);
XMLPUBFUN xmlChar *
		xmlTextWriterEndDTDAttlist	(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteDTDAttlist	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatDTDAttlist (xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatDTDAttlist (xmlTextWriterPtr writer,
						 const xmlChar *name,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN xmlChar *
		xmlTextWriterStartDTDEntity	(xmlTextWriterPtr writer,
					 int pe,
					 const xmlChar *name);
XMLPUBFUN xmlChar *
		xmlTextWriterEndDTDEntity	(xmlTextWriterPtr writer);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteDTDEntity	(xmlTextWriterPtr writer,
					 int pe,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid,
					 const xmlChar *ndataid,
					 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteDTDInternalEntity (xmlTextWriterPtr writer,
						 int pe,
						 const xmlChar *name,
						 const xmlChar *content);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteDTDExternalEntity (xmlTextWriterPtr writer,
						 int pe,
						 const xmlChar *name,
						 const xmlChar *pubid,
						 const xmlChar *sysid,
						 const xmlChar *ndataid);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteDTDExternalEntityContents (xmlTextWriterPtr writer,
						 const xmlChar *pubid,
						 const xmlChar *sysid,
						 const xmlChar *ndataid);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteDTDNotation	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatDTDInternalEntity (xmlTextWriterPtr writer,
						 int pe,
						 const xmlChar *name,
						 const char *format, ...) LIBXML_ATTR_FORMAT(4,5);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatDTDInternalEntity (xmlTextWriterPtr writer,
						 int pe,
						 const xmlChar *name,
						 const char *format,
						 va_list argptr) LIBXML_ATTR_FORMAT(4,0);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteFormatDTD	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid,
					 const char *format, ...) LIBXML_ATTR_FORMAT(5,6);
XMLPUBFUN xmlChar *
		xmlTextWriterWriteVFormatDTD	(xmlTextWriterPtr writer,
					 const xmlChar *name,
					 const xmlChar *pubid,
					 const xmlChar *sysid,
					 const char *format,
					 va_list argptr) LIBXML_ATTR_FORMAT(5,0);
XMLPUBFUN xmlChar *
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
