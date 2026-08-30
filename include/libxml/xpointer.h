/**
 * @file
 *
 * XPointer API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_XPTR_H__
#define __XML_XPTR_H__

#include <libxml/xmlversion.h>
#include <libxml/xpath.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Functions will be declared here as they are implemented. */


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN xmlXPathObject * xmlXPtrEval (const xmlChar *str, xmlXPathContext *ctx);
XMLPUBFUN xmlXPathContext * xmlXPtrNewContext (xmlDoc *doc, xmlNode *here, xmlNode *origin);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_XPTR_H__ */
