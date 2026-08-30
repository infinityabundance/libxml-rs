/**
 * @file
 *
 * XSLT variables API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __VARIABLES_H__
#define __VARIABLES_H__

#include <libxml/xmlversion.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/xslt.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Functions will be declared here as they are implemented. */


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XSLTPUBFUN int XSLTCALL xsltAddStackElemList (xsltTransformContextPtr ctxt, xsltStackElemPtr elems);
XSLTPUBFUN int XSLTCALL xsltEvalGlobalVariables (xsltTransformContextPtr ctxt);
XSLTPUBFUN int XSLTCALL xsltEvalOneUserParam (xsltTransformContextPtr ctxt, const xmlChar * name, const xmlChar * value);
XSLTPUBFUN int XSLTCALL xsltEvalUserParams (xsltTransformContextPtr ctxt, const char **params);
XSLTPUBFUN void XSLTCALL xsltFreeGlobalVariables (xsltTransformContextPtr ctxt);
XSLTPUBFUN void XSLTCALL xsltParseGlobalParam (xsltStylesheetPtr style, xmlNodePtr cur);
XSLTPUBFUN void XSLTCALL xsltParseGlobalVariable (xsltStylesheetPtr style, xmlNodePtr cur);
XSLTPUBFUN xsltStackElemPtr XSLTCALL xsltParseStylesheetCallerParam (xsltTransformContextPtr ctxt, xmlNodePtr cur);
XSLTPUBFUN void XSLTCALL xsltParseStylesheetParam (xsltTransformContextPtr ctxt, xmlNodePtr cur);
XSLTPUBFUN void XSLTCALL xsltParseStylesheetVariable (xsltTransformContextPtr ctxt, xmlNodePtr cur);
XSLTPUBFUN int XSLTCALL xsltQuoteOneUserParam (xsltTransformContextPtr ctxt, const xmlChar * name, const xmlChar * value);
XSLTPUBFUN int XSLTCALL xsltQuoteUserParams (xsltTransformContextPtr ctxt, const char **params);
XSLTPUBFUN xmlXPathObjectPtr XSLTCALL xsltVariableLookup (xsltTransformContextPtr ctxt, const xmlChar *name, const xmlChar *ns_uri);
XSLTPUBFUN xmlXPathObjectPtr XSLTCALL xsltXPathVariableLookup (void *ctxt, const xmlChar *name, const xmlChar *ns_uri);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __VARIABLES_H__ */
