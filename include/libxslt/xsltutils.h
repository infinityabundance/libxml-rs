/**
 * @file
 *
 * XSLT utilities for libxml-rs
 */

#ifndef __XSLTUTILS_H__
#define __XSLTUTILS_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <libxslt/xslt.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN void xsltSetTransformErrorFunc(xsltTransformContextPtr ctxt,
                                          void *ctx,
                                          xmlGenericErrorFunc handler);
XMLPUBFUN int xsltCheckFeature(int feature);

#ifdef __cplusplus
}
#endif

#endif /* __XSLTUTILS_H__ */
