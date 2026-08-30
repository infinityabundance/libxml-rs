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
#include <libxslt/xsltInternals.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN void xsltSetTransformErrorFunc(xsltTransformContextPtr ctxt,
                                          void *ctx,
                                          xmlGenericErrorFunc handler);
XMLPUBFUN int xsltCheckFeature(int feature);
XMLPUBFUN void xsltSetGenericErrorFunc(void *ctx, xmlGenericErrorFunc handler);
XMLPUBFUN void xsltTransformError(xsltTransformContextPtr ctxt,
                                  xsltStylesheetPtr style, xmlNodePtr node,
                                  const char *msg, ...);
XMLPUBFUN void xsltPrintErrorContext(xsltTransformContextPtr ctxt,
                                     xsltStylesheetPtr style, xmlNodePtr node);
XMLPUBFUN int xsltSetCtxtParseOptions(xsltTransformContextPtr ctxt, int options);
XMLPUBFUN int xsltGetDebuggerStatus(void);
XMLPUBFUN long xsltTimestamp(void);
XMLPUBFUN void xsltCalibrateAdjust(long delta);
XMLPUBFUN xmlDocPtr xsltGetProfileInformation(xsltTransformContextPtr ctxt);
XMLPUBFUN xmlDocPtr xsltProfileStylesheet(xsltStylesheetPtr style, xmlDocPtr doc,
                                          const char **params, FILE *output);
XMLPUBFUN void xsltSaveProfiling(xsltTransformContextPtr ctxt, FILE *output);
XMLPUBFUN int xsltSaveResultTo(xmlOutputBufferPtr buf, xmlDocPtr result,
                               xsltStylesheetPtr style);
XMLPUBFUN int xsltIsBlank(xmlChar *str);
XMLPUBFUN const xmlChar *xsltSplitQName(xmlDictPtr dict, const xmlChar *name,
                                        const xmlChar **prefix);
XMLPUBFUN const xmlChar *xsltGetQNameURI(xmlNodePtr node, xmlChar **name);
XMLPUBFUN int xsltGetUTF8Char(const unsigned char *utf, int *len);










































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef enum{
	XSLT_TRACE_ALL =		-1,
	XSLT_TRACE_NONE =		0,
	XSLT_TRACE_COPY_TEXT =		1<<0,
	XSLT_TRACE_PROCESS_NODE =	1<<1,
	XSLT_TRACE_APPLY_TEMPLATE =	1<<2,
	XSLT_TRACE_COPY =		1<<3,
	XSLT_TRACE_COMMENT =		1<<4,
	XSLT_TRACE_PI =			1<<5,
	XSLT_TRACE_COPY_OF =		1<<6,
	XSLT_TRACE_VALUE_OF =		1<<7,
	XSLT_TRACE_CALL_TEMPLATE =	1<<8,
	XSLT_TRACE_APPLY_TEMPLATES =	1<<9,
	XSLT_TRACE_CHOOSE =		1<<10,
	XSLT_TRACE_IF =			1<<11,
	XSLT_TRACE_FOR_EACH =		1<<12,
	XSLT_TRACE_STRIP_SPACES =	1<<13,
	XSLT_TRACE_TEMPLATES =		1<<14,
	XSLT_TRACE_KEYS =		1<<15,
	XSLT_TRACE_VARIABLES =		1<<16
} xsltDebugTraceCodes;

typedef enum{
    XSLT_DEBUG_NONE = 0, /* no debugging allowed */
    XSLT_DEBUG_INIT,
    XSLT_DEBUG_STEP,
    XSLT_DEBUG_STEPOUT,
    XSLT_DEBUG_NEXT,
    XSLT_DEBUG_STOP,
    XSLT_DEBUG_CONT,
    XSLT_DEBUG_RUN,
    XSLT_DEBUG_RUN_RESTART,
    XSLT_DEBUG_QUIT
} xsltDebugStatusCodes;

/* [11.1-G] end: extracted definitions */
#ifdef __cplusplus
}
#endif

#endif /* __XSLTUTILS_H__ */
