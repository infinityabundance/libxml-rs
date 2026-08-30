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

/* Extension-module registry (extensions.h 1.1.45). */
typedef void *(*xsltExtInitFunction)(xsltTransformContextPtr ctxt,
                                     const xmlChar *URI);
typedef void (*xsltExtShutdownFunction)(xsltTransformContextPtr ctxt,
                                        const xmlChar *URI, void *data);
typedef void *(*xsltStyleExtInitFunction)(xsltStylesheetPtr style,
                                          const xmlChar *URI);
typedef void (*xsltStyleExtShutdownFunction)(xsltStylesheetPtr style,
                                             const xmlChar *URI, void *data);
typedef void (*xsltTopLevelFunction)(xsltStylesheetPtr style, xmlNodePtr node,
                                     void *data);
XMLPUBFUN int xsltRegisterExtModule(const xmlChar *URI,
                                    xsltExtInitFunction initFunc,
                                    xsltExtShutdownFunction shutdownFunc);
XMLPUBFUN int xsltRegisterExtModuleFull(const xmlChar *URI,
                                        xsltExtInitFunction initFunc,
                                        xsltExtShutdownFunction shutdownFunc,
                                        xsltStyleExtInitFunction styleInitFunc,
                                        xsltStyleExtShutdownFunction styleShutdownFunc);
XMLPUBFUN int xsltRegisterExtModuleElement(const xmlChar *name, const xmlChar *URI,
                                           void *precomp, void *transform);
XMLPUBFUN int xsltRegisterExtModuleFunction(const xmlChar *name, const xmlChar *URI,
                                            xmlXPathFunction function);
XMLPUBFUN int xsltRegisterExtModuleTopLevel(const xmlChar *name, const xmlChar *URI,
                                            xsltTopLevelFunction function);
XMLPUBFUN int xsltUnregisterExtModule(const xmlChar *URI);
XMLPUBFUN int xsltUnregisterExtModuleElement(const xmlChar *name, const xmlChar *URI);
XMLPUBFUN int xsltUnregisterExtModuleFunction(const xmlChar *name, const xmlChar *URI);
XMLPUBFUN int xsltUnregisterExtModuleTopLevel(const xmlChar *name, const xmlChar *URI);
XMLPUBFUN int xsltRegisterExtPrefix(xsltStylesheetPtr style, const xmlChar *prefix,
                                    const xmlChar *URI);
XMLPUBFUN int xsltCheckExtPrefix(xsltStylesheetPtr style, const xmlChar *prefix);
XMLPUBFUN int xsltCheckExtURI(xsltStylesheetPtr style, const xmlChar *URI);
XMLPUBFUN void *xsltExtElementLookup(xsltTransformContextPtr ctxt,
                                     const xmlChar *name, const xmlChar *URI);
XMLPUBFUN void *xsltExtModuleElementLookup(const xmlChar *name, const xmlChar *URI);
XMLPUBFUN xmlXPathFunction xsltExtModuleFunctionLookup(const xmlChar *name,
                                                       const xmlChar *URI);
XMLPUBFUN void *xsltExtModuleElementPreComputeLookup(const xmlChar *name,
                                                     const xmlChar *URI);
XMLPUBFUN xsltTopLevelFunction xsltExtModuleTopLevelLookup(const xmlChar *name,
                                                           const xmlChar *URI);
XMLPUBFUN int xsltInitCtxtExts(xsltTransformContextPtr ctxt);
XMLPUBFUN void xsltShutdownCtxtExts(xsltTransformContextPtr ctxt);
XMLPUBFUN void xsltFreeCtxtExts(xsltTransformContextPtr ctxt);
XMLPUBFUN void *xsltGetExtData(xsltTransformContextPtr ctxt, const xmlChar *URI);
XMLPUBFUN void *xsltStyleGetExtData(xsltStylesheetPtr style, const xmlChar *URI);
XMLPUBFUN void *xsltGetExtInfo(xsltStylesheetPtr style, const xmlChar *URI);
XMLPUBFUN void xsltRegisterAllExtras(void);
XMLPUBFUN void xsltRegisterExtras(xsltTransformContextPtr ctxt);
XMLPUBFUN void xsltRegisterAllElement(xsltTransformContextPtr ctxt);
XMLPUBFUN void xsltRegisterTestModule(void);

#ifdef __cplusplus
}
#endif

#endif /* __EXTENSIONS_H__ */
