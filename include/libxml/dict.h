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


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN void xmlDictCleanup (void);
XMLPUBFUN int xmlDictOwns (xmlDict *dict, const xmlChar *str);
XMLPUBFUN const xmlChar * xmlDictQLookup (xmlDict *dict, const xmlChar *prefix, const xmlChar *name);
XMLPUBFUN int xmlDictReference(xmlDict *dict);
XMLPUBFUN int xmlInitializeDict(void);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_DICT_H__ */
