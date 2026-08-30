/**
 * @file
 *
 * XSLT pattern API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __PATTERN_H__
#define __PATTERN_H__

#include <libxml/xmlversion.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/xslt.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Compile-match types (upstream pattern.h) */
typedef struct _xsltCompMatch xsltCompMatch;
typedef xsltCompMatch *xsltCompMatchPtr;

/* Functions will be declared here as they are implemented. */


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XSLTPUBFUN int XSLTCALL xsltAddTemplate (xsltStylesheetPtr style, xsltTemplatePtr cur, const xmlChar *mode, const xmlChar *modeURI);
XSLTPUBFUN void XSLTCALL xsltCleanupTemplates (xsltStylesheetPtr style);
XSLTPUBFUN void XSLTCALL xsltCompMatchClearCache (xsltTransformContextPtr ctxt, xsltCompMatchPtr comp);
XSLTPUBFUN xsltCompMatchPtr XSLTCALL xsltCompilePattern (const xmlChar *pattern, xmlDocPtr doc, xmlNodePtr node, xsltStylesheetPtr style, xsltTransformContextPtr runtime);
XSLTPUBFUN void XSLTCALL xsltFreeCompMatchList (xsltCompMatchPtr comp);
XSLTPUBFUN void XSLTCALL xsltFreeTemplateHashes (xsltStylesheetPtr style);
XSLTPUBFUN xsltTemplatePtr XSLTCALL xsltGetTemplate (xsltTransformContextPtr ctxt, xmlNodePtr node, xsltStylesheetPtr style);
XSLTPUBFUN void XSLTCALL xsltNormalizeCompSteps (void *payload, void *data, const xmlChar *name);
XSLTPUBFUN int XSLTCALL xsltTestCompMatchList (xsltTransformContextPtr ctxt, xmlNodePtr node, xsltCompMatchPtr comp);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __PATTERN_H__ */
