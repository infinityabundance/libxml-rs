/**
 * @file
 *
 * XSLT imports API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __IMPORTS_H__
#define __IMPORTS_H__

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
XSLTPUBFUN int XSLTCALL xsltFindElemSpaceHandling(xsltTransformContextPtr ctxt, xmlNodePtr node);
XSLTPUBFUN xsltTemplatePtr XSLTCALL xsltFindTemplate (xsltTransformContextPtr ctxt, const xmlChar *name, const xmlChar *nameURI);
XSLTPUBFUN int XSLTCALL xsltNeedElemSpaceHandling(xsltTransformContextPtr ctxt);
XSLTPUBFUN xsltStylesheetPtr XSLTCALL xsltNextImport (xsltStylesheetPtr style);
XSLTPUBFUN int XSLTCALL xsltParseStylesheetImport(xsltStylesheetPtr style, xmlNodePtr cur);
XSLTPUBFUN int XSLTCALL xsltParseStylesheetInclude (xsltStylesheetPtr style, xmlNodePtr cur);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __IMPORTS_H__ */
