/**
 * @file
 *
 * XSLT extensions API for libxml-rs
 */

#ifndef __EXTENSIONS_H__
#define __EXTENSIONS_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xpath.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN int xsltRegisterExtFunction(xsltTransformContextPtr ctxt,
                                       const xmlChar *name, const xmlChar *NS_uri,
                                       xmlXPathFunction f);
XMLPUBFUN int xsltRegisterExtElement(xsltTransformContextPtr ctxt,
                                      const xmlChar *name, const xmlChar *NS_uri,
                                      void *f);
XMLPUBFUN void exsltRegisterAll(void);
XMLPUBFUN void xsltSetLoaderFunc(void *loader);

#ifdef __cplusplus
}
#endif

#endif /* __EXTENSIONS_H__ */
