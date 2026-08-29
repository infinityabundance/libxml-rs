/**
 * @file
 *
 * XSLT transform API for libxml-rs
 *
 * # UPSTREAM-PARITY
 * Function surface follows upstream `libxslt/transform.h` plus the
 * top-level engine entry points that upstream places in `xslt.h`
 * (`xsltParseStylesheetFile`, `xsltApplyStylesheet`, ...).
 */

#ifndef __TRANSFORM_H__
#define __TRANSFORM_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/parser.h>
#include <libxml/xmlIO.h>
#include <libxslt/xslt.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/xsltexports.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Top-level engine entry points (upstream xslt.h / transform.h) */
XMLPUBFUN int xsltLibxsltVersion(void);
XMLPUBFUN const char *xsltLibxsltVersionString(void);
XMLPUBFUN int xsltCheckVersion(int version);
XMLPUBFUN void xsltInit(void);
XMLPUBFUN void xsltCleanupGlobals(void);
XMLPUBFUN xsltStylesheetPtr xsltParseStylesheetFile(const xmlChar *filename);
XMLPUBFUN xsltStylesheetPtr xsltParseStylesheetDoc(xmlDocPtr doc);
XMLPUBFUN xsltStylesheetPtr xsltParseStylesheetMemory(const char *buf, int len,
                                                       const char *URL);
XMLPUBFUN void xsltFreeStylesheet(xsltStylesheetPtr style);
XMLPUBFUN xmlDocPtr xsltApplyStylesheet(xsltStylesheetPtr style, xmlDocPtr doc,
                                         const char **params);
XMLPUBFUN xmlDocPtr xsltApplyStylesheetUser(xsltStylesheetPtr style, xmlDocPtr doc,
                                             const char **params, const char *output,
                                             FILE *profile,
                                             xsltTransformContextPtr userCtxt);
XMLPUBFUN void xsltFreeTransformResult(xmlDocPtr result);
XMLPUBFUN xsltTransformContextPtr xsltNewTransformContext(xsltStylesheetPtr style,
                                                           xmlDocPtr doc);
XMLPUBFUN void xsltFreeTransformContext(xsltTransformContextPtr ctxt);
XMLPUBFUN int xsltSaveResultToFile(FILE *output, xmlDocPtr result,
                                    xsltStylesheetPtr style);
XMLPUBFUN int xsltSaveResultToFd(int fd, xmlDocPtr result,
                                  xsltStylesheetPtr style);
XMLPUBFUN int xsltSaveResultToString(xmlChar **doc_txt_ptr, int *doc_txt_len,
                                      xmlDocPtr result, xsltStylesheetPtr style);
XMLPUBFUN xmlDocPtr xsltGetStylesheetDoc(xsltStylesheetPtr style);
XMLPUBFUN void xsltSetStylesheetDoc(xsltStylesheetPtr style, xmlDocPtr doc);
XMLPUBFUN const char *xsltEngineVersion(void);

#ifdef __cplusplus
}
#endif

#endif /* __TRANSFORM_H__ */
