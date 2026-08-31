/**
 * @file
 *
 * Debug/dump API for libxml-rs
 */

#ifndef __DEBUG_XML_H__
#define __DEBUG_XML_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN void xmlDebugDumpDocument(FILE *output, xmlDocPtr doc);
XMLPUBFUN void xmlDebugDumpNode(FILE *output, xmlNodePtr node, int depth);
XMLPUBFUN void xmlDebugDumpNodeList(FILE *output, xmlNodePtr node, int depth);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN int xmlDebugCheckDocument (FILE * output, xmlDoc *doc);
XMLPUBFUN void xmlDebugDumpAttr (FILE *output, xmlAttr *attr, int depth);
XMLPUBFUN void xmlDebugDumpAttrList (FILE *output, xmlAttr *attr, int depth);
XMLPUBFUN void xmlDebugDumpDTD (FILE *output, xmlDtd *dtd);
XMLPUBFUN void xmlDebugDumpDocumentHead(FILE *output, xmlDoc *doc);
XMLPUBFUN void xmlDebugDumpEntities (FILE *output, xmlDoc *doc);
XMLPUBFUN void xmlDebugDumpOneNode (FILE *output, xmlNode *node, int depth);
XMLPUBFUN void xmlDebugDumpString (FILE *output, const xmlChar *str);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __DEBUG_XML_H__ */
