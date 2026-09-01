/**
 * @file
 *
 * XInclude API for libxml-rs
 */

#ifndef __XML_XINCLUDE_H__
#define __XML_XINCLUDE_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <libxml/parser.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Context type (upstream xinclude.h) */
typedef struct _xmlXIncludeCtxt xmlXIncludeCtxt;
typedef xmlXIncludeCtxt *xmlXIncludeCtxtPtr;

/**
 * Macro defining the Xinclude namespace: http://www.w3.org/2003/XInclude
 */
#define XINCLUDE_NS (const xmlChar *) "http://www.w3.org/2003/XInclude"
/**
 * Macro defining the draft Xinclude namespace: http://www.w3.org/2001/XInclude
 */
#define XINCLUDE_OLD_NS (const xmlChar *) "http://www.w3.org/2001/XInclude"
/**
 * Macro defining "include"
 */
#define XINCLUDE_NODE (const xmlChar *) "include"
/**
 * Macro defining "fallback"
 */
#define XINCLUDE_FALLBACK (const xmlChar *) "fallback"
/**
 * Macro defining "href"
 */
#define XINCLUDE_HREF (const xmlChar *) "href"
/**
 * Macro defining "parse"
 */
#define XINCLUDE_PARSE (const xmlChar *) "parse"
/**
 * Macro defining "xml"
 */
#define XINCLUDE_PARSE_XML (const xmlChar *) "xml"
/**
 * Macro defining "text"
 */
#define XINCLUDE_PARSE_TEXT (const xmlChar *) "text"
/**
 * Macro defining "encoding"
 */
#define XINCLUDE_PARSE_ENCODING (const xmlChar *) "encoding"
/**
 * Macro defining "xpointer"
 */
#define XINCLUDE_PARSE_XPOINTER (const xmlChar *) "xpointer"

XMLPUBFUN int xmlXIncludeProcess(xmlDocPtr doc);
XMLPUBFUN int xmlXIncludeProcessFlags(xmlDocPtr doc, int flags);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN void xmlXIncludeFreeContext (xmlXIncludeCtxt *ctxt);
XMLPUBFUN int xmlXIncludeGetLastError (xmlXIncludeCtxt *ctxt);
XMLPUBFUN xmlXIncludeCtxt * xmlXIncludeNewContext (xmlDoc *doc);
XMLPUBFUN int xmlXIncludeProcessFlagsData(xmlDoc *doc, int flags, void *data);
XMLPUBFUN int xmlXIncludeProcessNode (xmlXIncludeCtxt *ctxt, xmlNode *tree);
XMLPUBFUN int xmlXIncludeProcessTree (xmlNode *tree);
XMLPUBFUN int xmlXIncludeProcessTreeFlags(xmlNode *tree, int flags);
XMLPUBFUN int xmlXIncludeProcessTreeFlagsData(xmlNode *tree, int flags, void *data);
XMLPUBFUN void xmlXIncludeSetErrorHandler(xmlXIncludeCtxt *ctxt, xmlStructuredErrorFunc handler, void *data);
XMLPUBFUN int xmlXIncludeSetFlags (xmlXIncludeCtxt *ctxt, int flags);
XMLPUBFUN void xmlXIncludeSetResourceLoader(xmlXIncludeCtxt *ctxt, xmlResourceLoader loader, void *data);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_XINCLUDE_H__ */
