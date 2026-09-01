/**
 * @file
 *
 * Global state API for libxml-rs
 */

#ifndef __XML_GLOBALS_H__
#define __XML_GLOBALS_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlerror.h>
#include <libxml/parser.h>
#include <libxml/HTMLparser.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN void xmlInitGlobals(void);
XMLPUBFUN void xmlCleanupGlobals(void);

/* Exported data globals (upstream globals.h XMLPUBVAR declarations).
 *
 * Phase 13 (HOSTILE-THREADS): the parser defaults, the error-handler slots
 * and the node/IO hooks are THREAD-LOCAL in upstream 2.15
 * (LIBXML_THREAD_ENABLED, globals.c xmlGetThreadLocalStorage): the oracle
 * headers declare them as `(*__xmlXxx())` macro aliases of accessor
 * FUNCTIONS (parser.h / xmlerror.h / xmlIO.h / tree.h), and the oracle DSO
 * exports only those accessors. The candidate headers follow the same
 * contract, so the declarations moved to the matching headers as accessor
 * decls + macros; only the globals the executed oracle keeps as plain data
 * remain declared here. */
XMLPUBVAR int xmlParserDebugEntities;
XMLPUBVAR int xmlTreeIndentNumber;
XMLPUBVAR xmlBufferAllocationScheme xmlBufferAllocScheme;
XMLPUBVAR int xmlDefaultBufferSize;
XMLPUBVAR int xmlParserMaxDepth;

#ifdef __cplusplus
}
#endif

#endif /* __XML_GLOBALS_H__ */
