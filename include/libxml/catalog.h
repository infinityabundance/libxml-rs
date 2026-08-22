/**
 * @file
 *
 * Catalog API for libxml-rs
 */

#ifndef __XML_CATALOG_H__
#define __XML_CATALOG_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Catalog allow values */
typedef enum {
    XML_CATA_ALLOW_NONE = 0,
    XML_CATA_ALLOW_GLOBAL = 1,
    XML_CATA_ALLOW_DOCUMENT = 2,
    XML_CATA_ALLOW_ALL = 3
} xmlCatalogAllow;
typedef int xmlCatalogAllowValue;

XMLPUBFUN void *xmlCatalogLoad(const char *catalogs);
XMLPUBFUN xmlChar *xmlCatalogResolvePublic(const xmlChar *pubID);
XMLPUBFUN xmlChar *xmlCatalogResolveSystem(const xmlChar *sysID);
XMLPUBFUN xmlChar *xmlCatalogResolveURI(const xmlChar *URI);
XMLPUBFUN void xmlCatalogSetDefaults(xmlCatalogAllowValue allow);
XMLPUBFUN xmlCatalogAllowValue xmlCatalogGetDefaults(void);
XMLPUBFUN int xmlCatalogAdd(const xmlChar *type, const xmlChar *orig,
                             const xmlChar *replace);
XMLPUBFUN int xmlCatalogRemove(const xmlChar *value);
XMLPUBFUN void xmlCatalogCleanup(void);
XMLPUBFUN xmlDocPtr xmlCatalogConvert(void);

#ifdef __cplusplus
}
#endif

#endif /* __XML_CATALOG_H__ */
