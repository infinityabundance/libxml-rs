/**
 * @file
 *
 * XPath internals API for libxml-rs (11.1-H header-surface closure).
 *
 * Upstream counterpart: include/libxml/xpathInternals.h
 * This header declares the subset of the upstream XPath-internals surface that
 * libxml-rs exports, with upstream-compatible signatures. Every declared
 * function exists in the candidate DSO (the header is honest by construction).
 * Declarations for upstream functions not yet exported are tracked as parity
 * obligations (11.1-I), not silently omitted.
 */

#ifndef __XML_XPATH_INTERNALS_H__
#define __XML_XPATH_INTERNALS_H__

#include <libxml/xmlversion.h>
#include <libxml/xpath.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Function-pointer type for XPath extension functions (upstream parity).
 * Declared in libxml/xpath.h (upstream location); this header does not
 * redefine it. The candidate registers an ABI-compatible trampoline. */
#include <libxml/xpath.h>

XMLPUBFUN xmlXPathContextPtr
                xmlXPathNewContext         (xmlDocPtr doc);
XMLPUBFUN void
                xmlXPathFreeContext        (xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathEvalExpression     (const xmlChar *str,
                                            xmlXPathContextPtr ctxt);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathEval               (const xmlChar *str,
                                            xmlXPathContextPtr ctxt);
XMLPUBFUN void
                xmlXPathFreeObject         (xmlXPathObjectPtr obj);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathObjectCopy         (xmlXPathObjectPtr val);
XMLPUBFUN xmlChar *
                xmlXPathCastToString       (xmlXPathObjectPtr val);
XMLPUBFUN double
                xmlXPathCastStringToNumber (const xmlChar *val);
XMLPUBFUN int
                xmlXPathCmpNodes           (xmlNodePtr node1,
                                            xmlNodePtr node2);
XMLPUBFUN xmlNodeSetPtr
                xmlXPathNodeSetCreate      (xmlNodePtr val);
XMLPUBFUN void
                xmlXPathFreeNodeSet        (xmlNodeSetPtr ns);
XMLPUBFUN void *
                xmlXPathCompile            (const xmlChar *str);
XMLPUBFUN void
                xmlXPathFreeCompExpr       (void *comp);
XMLPUBFUN int
                xmlXPathRegisterNs         (xmlXPathContextPtr ctxt,
                                            const xmlChar *prefix,
                                            const xmlChar *ns_uri);
XMLPUBFUN int
                xmlXPathRegisterFunc       (xmlXPathContextPtr ctxt,
                                            const xmlChar *name,
                                            xmlXPathFunction f);
XMLPUBFUN int
                xmlXPathRegisterFuncNS     (xmlXPathContextPtr ctxt,
                                            const xmlChar *name,
                                            const xmlChar *ns_uri,
                                            xmlXPathFunction f);
XMLPUBFUN int
                xmlXPathRegisterVariable   (xmlXPathContextPtr ctxt,
                                            const xmlChar *name,
                                            xmlXPathObjectPtr value);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathNewNodeSet         (xmlNodePtr val);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathNewCString         (const xmlChar *val);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathNewFloat           (double val);
XMLPUBFUN xmlXPathObjectPtr
                xmlXPathNewBoolean         (int val);

/* Upstream macros preserved for source compatibility (11.1-H). */
#define xmlXPathNodeSetIsEmpty(ns) ((ns) == NULL ? 1 : ((ns)->nodeNr == 0))
#define xmlXPathNodeSetGetLength(ns) ((ns) == NULL ? 0 : ((ns)->nodeNr))
#define xmlXPathNodeSetItem(ns, index) \
    (((ns) != NULL && (index) >= 0 && (index) < (ns)->nodeNr) ? \
     (ns)->nodeTab[(index)] : NULL)

#ifdef __cplusplus
}
#endif

#endif /* __XML_XPATH_INTERNALS_H__ */
