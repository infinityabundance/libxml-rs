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
