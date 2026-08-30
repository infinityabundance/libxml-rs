/**
 * @file
 *
 * XSLT templates API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __TEMPLATES_H__
#define __TEMPLATES_H__

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
XSLTPUBFUN xmlAttrPtr XSLTCALL xsltAttrListTemplateProcess (xsltTransformContextPtr ctxt, xmlNodePtr target, xmlAttrPtr cur);
XSLTPUBFUN xmlAttrPtr XSLTCALL xsltAttrTemplateProcess (xsltTransformContextPtr ctxt, xmlNodePtr target, xmlAttrPtr attr);
XSLTPUBFUN xmlChar * XSLTCALL xsltAttrTemplateValueProcess (xsltTransformContextPtr ctxt, const xmlChar* attr);
XSLTPUBFUN xmlChar * XSLTCALL xsltAttrTemplateValueProcessNode(xsltTransformContextPtr ctxt, const xmlChar* str, xmlNodePtr node);
XSLTPUBFUN xmlChar * XSLTCALL xsltEvalAttrValueTemplate (xsltTransformContextPtr ctxt, xmlNodePtr node, const xmlChar *name, const xmlChar *ns);
XSLTPUBFUN const xmlChar * XSLTCALL xsltEvalStaticAttrValueTemplate (xsltStylesheetPtr style, xmlNodePtr node, const xmlChar *name, const xmlChar *ns, int *found);
XSLTPUBFUN xmlChar * XSLTCALL xsltEvalTemplateString (xsltTransformContextPtr ctxt, xmlNodePtr contextNode, xmlNodePtr inst);
XSLTPUBFUN int XSLTCALL xsltEvalXPathPredicate (xsltTransformContextPtr ctxt, xmlXPathCompExprPtr comp, xmlNsPtr *nsList, int nsNr);
XSLTPUBFUN xmlChar * XSLTCALL xsltEvalXPathString (xsltTransformContextPtr ctxt, xmlXPathCompExprPtr comp);
XSLTPUBFUN xmlChar * XSLTCALL xsltEvalXPathStringNs (xsltTransformContextPtr ctxt, xmlXPathCompExprPtr comp, int nsNr, xmlNsPtr *nsList);
XSLTPUBFUN xmlNodePtr * XSLTCALL xsltTemplateProcess (xsltTransformContextPtr ctxt, xmlNodePtr node);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __TEMPLATES_H__ */
