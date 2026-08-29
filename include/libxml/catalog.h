/**
 * @file
 *
 * Catalog API (libxml-rs). Mirrors upstream libxml2 2.15.3 catalog.h.
 */

#ifndef __XML_CATALOG_H__
#define __XML_CATALOG_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlCatalog xmlCatalog;
typedef xmlCatalog *xmlCatalogPtr;

/* Catalog allow values */
typedef enum {
    XML_CATA_ALLOW_NONE = 0,
    XML_CATA_ALLOW_GLOBAL = 1,
    XML_CATA_ALLOW_DOCUMENT = 2,
    XML_CATA_ALLOW_ALL = 3
} xmlCatalogAllow;

typedef enum {
    XML_CATA_PREFER_NONE = 0,
    XML_CATA_PREFER_PUBLIC = 1,
    XML_CATA_PREFER_SYSTEM = 2
} xmlCatalogPrefer;

/* Operations on a given catalog. */
XMLPUBFUN xmlCatalog *xmlNewCatalog(int sgml);
XMLPUBFUN xmlCatalog *xmlLoadACatalog(const char *filename);
XMLPUBFUN xmlCatalog *xmlLoadSGMLSuperCatalog(const char *filename);
XMLPUBFUN int xmlConvertSGMLCatalog(xmlCatalog *catal);
XMLPUBFUN int xmlACatalogAdd(xmlCatalog *catal, const xmlChar *type,
                              const xmlChar *orig, const xmlChar *replace);
XMLPUBFUN int xmlACatalogRemove(xmlCatalog *catal, const xmlChar *value);
XMLPUBFUN xmlChar *xmlACatalogResolve(xmlCatalog *catal, const xmlChar *pubID,
                                      const xmlChar *sysID);
XMLPUBFUN xmlChar *xmlACatalogResolveSystem(xmlCatalog *catal,
                                             const xmlChar *sysID);
XMLPUBFUN xmlChar *xmlACatalogResolvePublic(xmlCatalog *catal,
                                             const xmlChar *pubID);
XMLPUBFUN xmlChar *xmlACatalogResolveURI(xmlCatalog *catal,
                                          const xmlChar *URI);
XMLPUBFUN void xmlACatalogDump(xmlCatalog *catal, FILE *out);
XMLPUBFUN void xmlFreeCatalog(xmlCatalog *catal);
XMLPUBFUN int xmlCatalogIsEmpty(xmlCatalog *catal);

/* Global operations. */
XMLPUBFUN void xmlInitializeCatalog(void);
XMLPUBFUN int xmlLoadCatalog(const char *filename);
XMLPUBFUN void xmlLoadCatalogs(const char *paths);
XMLPUBFUN void xmlCatalogCleanup(void);
XMLPUBFUN void xmlCatalogDump(FILE *out);
XMLPUBFUN xmlDocPtr xmlCatalogDumpDoc(void);
XMLPUBFUN xmlChar *xmlCatalogResolve(const xmlChar *pubID,
                                     const xmlChar *sysID);
XMLPUBFUN xmlChar *xmlCatalogResolveSystem(const xmlChar *sysID);
XMLPUBFUN xmlChar *xmlCatalogResolvePublic(const xmlChar *pubID);
XMLPUBFUN xmlChar *xmlCatalogResolveURI(const xmlChar *URI);
XMLPUBFUN int xmlCatalogAdd(const xmlChar *type, const xmlChar *orig,
                             const xmlChar *replace);
XMLPUBFUN int xmlCatalogRemove(const xmlChar *value);
XMLPUBFUN xmlDoc *xmlParseCatalogFile(const char *filename);
XMLPUBFUN int xmlCatalogConvert(void);

/* Per-document local catalogs. */
XMLPUBFUN void xmlCatalogFreeLocal(void *catalogs);
XMLPUBFUN void *xmlCatalogAddLocal(void *catalogs, const xmlChar *URL);
XMLPUBFUN xmlChar *xmlCatalogLocalResolve(void *catalogs,
                                          const xmlChar *pubID,
                                          const xmlChar *sysID);
XMLPUBFUN xmlChar *xmlCatalogLocalResolveURI(void *catalogs,
                                              const xmlChar *URI);

/* Preference settings. */
XMLPUBFUN int xmlCatalogSetDebug(int level);
XMLPUBFUN xmlCatalogPrefer xmlCatalogSetDefaultPrefer(xmlCatalogPrefer prefer);
XMLPUBFUN void xmlCatalogSetDefaults(xmlCatalogAllow allow);
XMLPUBFUN xmlCatalogAllow xmlCatalogGetDefaults(void);

/* Deprecated interfaces. */
XMLPUBFUN const xmlChar *xmlCatalogGetSystem(const xmlChar *sysID);
XMLPUBFUN const xmlChar *xmlCatalogGetPublic(const xmlChar *pubID);

#ifdef __cplusplus
}
#endif

#endif /* __XML_CATALOG_H__ */
