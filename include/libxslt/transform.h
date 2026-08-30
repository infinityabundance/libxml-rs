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


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XSLTPUBFUN void XSLTCALL xsltApplyImports (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltApplyOneTemplate (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr list, xsltTemplatePtr templ, xsltStackElemPtr params);
XSLTPUBFUN void XSLTCALL xsltApplyStripSpaces (xsltTransformContextPtr ctxt, xmlNodePtr node);
XSLTPUBFUN void XSLTCALL xsltApplyTemplates (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltAttribute (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltCallTemplate (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltChoose (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltComment (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltCopy (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltCopyOf (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN xmlNodePtr XSLTCALL xsltCopyTextString (xsltTransformContextPtr ctxt, xmlNodePtr target, const xmlChar *string, int noescape);
XSLTPUBFUN void XSLTCALL xsltDocumentElem (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltElement (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltForEach (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN int XSLTCALL xsltGetXIncludeDefault (void);
XSLTPUBFUN void XSLTCALL xsltIf (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltLocalVariablePop (xsltTransformContextPtr ctxt, int limitNr, int level);
XSLTPUBFUN int XSLTCALL xsltLocalVariablePush (xsltTransformContextPtr ctxt, xsltStackElemPtr variable, int level);
XSLTPUBFUN void XSLTCALL xsltNumber (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltProcessOneNode (xsltTransformContextPtr ctxt, xmlNodePtr node, xsltStackElemPtr params);
XSLTPUBFUN void XSLTCALL xsltProcessingInstruction(xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN int XSLTCALL xsltRunStylesheet (xsltStylesheetPtr style, xmlDocPtr doc, const char **params, const char *output, xmlSAXHandlerPtr SAX, xmlOutputBufferPtr IObuf);
XSLTPUBFUN int XSLTCALL xsltRunStylesheetUser (xsltStylesheetPtr style, xmlDocPtr doc, const char **params, const char *output, xmlSAXHandlerPtr SAX, xmlOutputBufferPtr IObuf, FILE * profile, xsltTransformContextPtr userCtxt);
XSLTPUBFUN void XSLTCALL xsltSetXIncludeDefault (int xinclude);
XSLTPUBFUN void XSLTCALL xsltSort (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltText (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
XSLTPUBFUN void XSLTCALL xsltValueOf (xsltTransformContextPtr ctxt, xmlNodePtr node, xmlNodePtr inst, xsltElemPreCompPtr comp);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __TRANSFORM_H__ */
