/**
 * @file
 *
 * XSLT document API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __DOCUMENTS_H__
#define __DOCUMENTS_H__

#include <libxml/xmlversion.h>
#include <libxslt/xsltInternals.h>
#include <libxslt/xslt.h>

#ifdef __cplusplus
extern "C" {
#endif






























/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef enum{
    XSLT_LOAD_START = 0,	/* loading for a top stylesheet */
    XSLT_LOAD_STYLESHEET = 1,	/* loading for a stylesheet include/import */
    XSLT_LOAD_DOCUMENT = 2	/* loading document at transformation time */
} xsltLoadType;

/* [11.1-G] end: extracted definitions */

/* Document loader callback (upstream documents.h) */
typedef xmlDocPtr (*xsltDocLoaderFunc) (const xmlChar *URI,
					 xmlDictPtr dict,
					 int options,
					 void *ctxt,
					 xsltLoadType type);

/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XSLTPUBVAR xsltDocLoaderFunc xsltDocDefaultLoader;
XSLTPUBFUN xsltDocumentPtr XSLTCALL xsltFindDocument (xsltTransformContextPtr ctxt, xmlDocPtr doc);
XSLTPUBFUN void XSLTCALL xsltFreeDocuments (xsltTransformContextPtr ctxt);
XSLTPUBFUN void XSLTCALL xsltFreeStyleDocuments (xsltStylesheetPtr style);
XSLTPUBFUN xsltDocumentPtr XSLTCALL xsltLoadDocument (xsltTransformContextPtr ctxt, const xmlChar *URI);
XSLTPUBFUN xsltDocumentPtr XSLTCALL xsltLoadStyleDocument (xsltStylesheetPtr style, const xmlChar *URI);
XSLTPUBFUN xsltDocumentPtr XSLTCALL xsltNewDocument (xsltTransformContextPtr ctxt, xmlDocPtr doc);
XSLTPUBFUN xsltDocumentPtr XSLTCALL xsltNewStyleDocument (xsltStylesheetPtr style, xmlDocPtr doc);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __DOCUMENTS_H__ */
