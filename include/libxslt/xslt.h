/**
 * @file
 *
 * XSLT main header for libxml-rs
 */

#ifndef __XSLT_H__
#define __XSLT_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/* XSLT version */
#define LIBXSLT_DOTTED_VERSION "1.1.39"
#define LIBXSLT_VERSION 10139
#define LIBXSLT_VERSION_STRING "10139"
#define LIBXSLT_VERSION_EXTRA ""

/* Stylesheet type */
typedef struct _xsltStylesheet xsltStylesheet;
typedef xsltStylesheet *xsltStylesheetPtr;

/* Transform context */
typedef struct _xsltTransformContext xsltTransformContext;
typedef xsltTransformContext *xsltTransformContextPtr;

/* XSLT functions */
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

#endif /* __XSLT_H__ */
