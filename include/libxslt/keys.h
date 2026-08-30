/**
 * @file
 *
 * XSLT keys API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __KEYS_H__
#define __KEYS_H__

#include <libxml/xmlversion.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Functions will be declared here as they are implemented. */

XMLPUBFUN int xsltAddKey(xsltStylesheetPtr style, const xmlChar *name,
                         const xmlChar *nameURI, const xmlChar *match,
                         const xmlChar *use, xmlNodePtr inst);
XMLPUBFUN void xsltInitCtxtKeys(xsltTransformContextPtr ctxt,
                                xsltDocumentPtr idoc);
XMLPUBFUN int xsltInitCtxtKey(xsltTransformContextPtr ctxt,
                              xsltDocumentPtr idoc, xsltKeyDefPtr keyDef);
XMLPUBFUN int xsltInitAllDocKeys(xsltTransformContextPtr ctxt);
XMLPUBFUN xmlNodeSetPtr xsltGetKey(xsltTransformContextPtr ctxt,
                                   const xmlChar *name, const xmlChar *nameURI,
                                   const xmlChar *value);
XMLPUBFUN void xsltFreeKeys(xsltStylesheetPtr style);
XMLPUBFUN void xsltFreeDocumentKeys(xsltDocumentPtr idoc);

#ifdef __cplusplus
}
#endif

#endif /* __KEYS_H__ */
