/**
 * @file
 *
 * XSLT functions API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __FUNCTIONS_H__
#define __FUNCTIONS_H__

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
XSLTPUBFUN void XSLTCALL xsltDocumentFunction (xmlXPathParserContextPtr ctxt, int nargs);
XSLTPUBFUN void XSLTCALL xsltElementAvailableFunction (xmlXPathParserContextPtr ctxt, int nargs);
XSLTPUBFUN void XSLTCALL xsltFormatNumberFunction (xmlXPathParserContextPtr ctxt, int nargs);
XSLTPUBFUN void XSLTCALL xsltFunctionAvailableFunction (xmlXPathParserContextPtr ctxt, int nargs);
XSLTPUBFUN void XSLTCALL xsltGenerateIdFunction (xmlXPathParserContextPtr ctxt, int nargs);
XSLTPUBFUN void XSLTCALL xsltKeyFunction (xmlXPathParserContextPtr ctxt, int nargs);
XSLTPUBFUN void XSLTCALL xsltRegisterAllFunctions (xmlXPathContextPtr ctxt);
XSLTPUBFUN void XSLTCALL xsltSystemPropertyFunction (xmlXPathParserContextPtr ctxt, int nargs);
XSLTPUBFUN void XSLTCALL xsltUnparsedEntityURIFunction (xmlXPathParserContextPtr ctxt, int nargs);
XSLTPUBFUN xmlXPathFunction XSLTCALL xsltXPathFunctionLookup (void *vctxt, const xmlChar *name, const xmlChar *ns_uri);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __FUNCTIONS_H__ */
