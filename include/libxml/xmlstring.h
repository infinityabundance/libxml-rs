/**
 * @file
 *
 * String utility functions for libxml-rs
 */

#ifndef __XML_STRING_H__
#define __XML_STRING_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned char xmlChar;

XMLPUBFUN xmlChar *xmlStrdup(const xmlChar *cur);
XMLPUBFUN xmlChar *xmlStrndup(const xmlChar *cur, int len);
XMLPUBFUN int xmlStrlen(const xmlChar *str);
XMLPUBFUN int xmlStrcmp(const xmlChar *str1, const xmlChar *str2);
XMLPUBFUN int xmlStrncmp(const xmlChar *str1, const xmlChar *str2, int len);
XMLPUBFUN int xmlStrcasecmp(const xmlChar *str1, const xmlChar *str2);
XMLPUBFUN int xmlStrncasecmp(const xmlChar *str1, const xmlChar *str2, int len);
XMLPUBFUN int xmlStrEqual(const xmlChar *str1, const xmlChar *str2);
XMLPUBFUN int xmlStrQEqual(const xmlChar *pref, const xmlChar *name, const xmlChar *str);
XMLPUBFUN xmlChar *xmlStrcat(xmlChar *cur, const xmlChar *add);
XMLPUBFUN xmlChar *xmlStrncat(xmlChar *cur, const xmlChar *add, int len);
XMLPUBFUN xmlChar *xmlStrncatNew(const xmlChar *str1, const xmlChar *str2, int len);
XMLPUBFUN xmlChar *xmlStrcpy(xmlChar *dst, const xmlChar *src);
XMLPUBFUN xmlChar *xmlStrncpy(xmlChar *dst, const xmlChar *src, int len);
XMLPUBFUN xmlChar *xmlStrsub(const xmlChar *str, int start, int len);

#ifdef __cplusplus
}
#endif

#endif /* __XML_STRING_H__ */
