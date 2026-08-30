/**
 * @file
 *
 * XSLT namespace alias API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __NAMESPACES_H__
#define __NAMESPACES_H__

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
XSLTPUBFUN xmlNsPtr XSLTCALL xsltCopyNamespace (xsltTransformContextPtr ctxt, xmlNodePtr elem, xmlNsPtr ns);
XSLTPUBFUN xmlNsPtr XSLTCALL xsltCopyNamespaceList (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNsPtr cur);
XSLTPUBFUN void XSLTCALL xsltFreeNamespaceAliasHashes (xsltStylesheetPtr style);
XSLTPUBFUN xmlNsPtr XSLTCALL xsltGetNamespace (xsltTransformContextPtr ctxt, xmlNodePtr cur, xmlNsPtr ns, xmlNodePtr out);
XSLTPUBFUN xmlNsPtr XSLTCALL xsltGetPlainNamespace (xsltTransformContextPtr ctxt, xmlNodePtr cur, xmlNsPtr ns, xmlNodePtr out);
XSLTPUBFUN xmlNsPtr XSLTCALL xsltGetSpecialNamespace (xsltTransformContextPtr ctxt, xmlNodePtr cur, const xmlChar *URI, const xmlChar *prefix, xmlNodePtr out);
XSLTPUBFUN void XSLTCALL xsltNamespaceAlias (xsltStylesheetPtr style, xmlNodePtr node);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __NAMESPACES_H__ */
