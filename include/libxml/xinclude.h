/**
 * @file
 *
 * XInclude API for libxml-rs
 */

#ifndef __XML_XINCLUDE_H__
#define __XML_XINCLUDE_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN int xmlXIncludeProcess(xmlDocPtr doc);
XMLPUBFUN int xmlXIncludeProcessFlags(xmlDocPtr doc, int flags);

#ifdef __cplusplus
}
#endif

#endif /* __XML_XINCLUDE_H__ */
