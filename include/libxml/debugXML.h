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
XMLPUBFUN void xmlDebugDumpNode(FILE *output, xmlNodePtr node);
XMLPUBFUN void xmlDebugDumpNodeList(FILE *output, xmlNodePtr node);

#ifdef __cplusplus
}
#endif

#endif /* __DEBUG_XML_H__ */
