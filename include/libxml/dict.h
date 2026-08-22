/**
 * @file
 *
 * Dictionary API for libxml-rs
 */

#ifndef __XML_DICT_H__
#define __XML_DICT_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlDict xmlDict;
typedef xmlDict *xmlDictPtr;

XMLPUBFUN xmlDictPtr xmlDictCreate(void);
XMLPUBFUN xmlDictPtr xmlDictCreateSub(xmlDictPtr sub);
XMLPUBFUN const xmlChar *xmlDictLookup(xmlDictPtr dict, const xmlChar *name, int len);
XMLPUBFUN const xmlChar *xmlDictExists(xmlDictPtr dict, const xmlChar *name, int len);
XMLPUBFUN unsigned int xmlDictSize(const xmlDictPtr dict);
XMLPUBFUN void xmlDictFree(xmlDictPtr dict);
XMLPUBFUN unsigned int xmlDictSetLimit(xmlDictPtr dict, unsigned int limit);
XMLPUBFUN unsigned int xmlDictGetUsage(const xmlDictPtr dict);

#ifdef __cplusplus
}
#endif

#endif /* __XML_DICT_H__ */
