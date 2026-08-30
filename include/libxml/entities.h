/**
 * @file
 *
 * Entity handling API for libxml-rs
 */

#ifndef __XML_ENTITIES_H__
#define __XML_ENTITIES_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Entity functions are declared in tree.h */


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN xmlEntity * xmlAddDocEntity (xmlDoc *doc, const xmlChar *name, int type, const xmlChar *publicId, const xmlChar *systemId, const xmlChar *content);
XMLPUBFUN xmlEntity * xmlAddDtdEntity (xmlDoc *doc, const xmlChar *name, int type, const xmlChar *publicId, const xmlChar *systemId, const xmlChar *content);
XMLPUBFUN int xmlAddEntity (xmlDoc *doc, int extSubset, const xmlChar *name, int type, const xmlChar *publicId, const xmlChar *systemId, const xmlChar *content, xmlEntity **out);
XMLPUBFUN xmlEntitiesTable * xmlCopyEntitiesTable (xmlEntitiesTable *table);
XMLPUBFUN xmlEntitiesTable * xmlCreateEntitiesTable (void);
XMLPUBFUN void xmlDumpEntitiesTable (xmlBuffer *buf, xmlEntitiesTable *table);
XMLPUBFUN void xmlDumpEntityDecl (xmlBuffer *buf, xmlEntity *ent);
XMLPUBFUN xmlChar * xmlEncodeEntitiesReentrant(xmlDoc *doc, const xmlChar *input);
XMLPUBFUN xmlChar * xmlEncodeSpecialChars (const xmlDoc *doc, const xmlChar *input);
XMLPUBFUN void xmlFreeEntitiesTable (xmlEntitiesTable *table);
XMLPUBFUN void xmlFreeEntity (xmlEntity *entity);
XMLPUBFUN xmlEntity * xmlGetDtdEntity (xmlDoc *doc, const xmlChar *name);
XMLPUBFUN xmlEntity * xmlGetPredefinedEntity (const xmlChar *name);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_ENTITIES_H__ */
