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

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN void xmlInitGlobals(void);
XMLPUBFUN void xmlCleanupGlobals(void);

/* Exported data globals (upstream globals.h XMLPUBVAR declarations). */
XMLPUBVAR int xmlDoValidityCheckingDefaultValue;
XMLPUBVAR int xmlGetWarningsDefaultValue;
XMLPUBVAR int xmlLoadExtDtdDefaultValue;
XMLPUBVAR int xmlPedanticParserDefaultValue;
XMLPUBVAR int xmlKeepBlanksDefaultValue;
XMLPUBVAR int xmlLineNumbersDefaultValue;
XMLPUBVAR int xmlSubstituteEntitiesDefaultValue;
XMLPUBVAR int xmlParserDebugEntities;
XMLPUBVAR int xmlIndentTreeOutput;
XMLPUBVAR int xmlTreeIndentNumber;
XMLPUBVAR const char *xmlTreeIndentString;
XMLPUBVAR int xmlSaveNoEmptyTags;
XMLPUBVAR xmlGenericErrorFunc xmlGenericError;
XMLPUBVAR void *xmlGenericErrorContext;
XMLPUBVAR xmlStructuredErrorFunc xmlStructuredError;
XMLPUBVAR void *xmlStructuredErrorContext;
XMLPUBVAR xmlBufferAllocationScheme xmlBufferAllocScheme;
XMLPUBVAR int xmlDefaultBufferSize;
XMLPUBVAR int xmlParserMaxDepth;

#ifdef __cplusplus
}
#endif

#endif /* __XML_GLOBALS_H__ */
