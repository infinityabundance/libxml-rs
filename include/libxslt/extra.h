/**
 * @file
 *
 * XSLT extra API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __EXTRA_H__
#define __EXTRA_H__

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
XSLTPUBFUN void XSLTCALL xsltDebug (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltFunctionNodeSet (xmlXPathParserContextPtr ctxt, int nargs);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __EXTRA_H__ */
