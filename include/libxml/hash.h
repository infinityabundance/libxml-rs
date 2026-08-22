/**
 * @file
 *
 * Hash table API for libxml-rs
 */

#ifndef __XML_HASH_H__
#define __XML_HASH_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlHashTable xmlHashTable;
typedef xmlHashTable *xmlHashTablePtr;

typedef void (*xmlHashDeallocator)(void *payload, xmlChar *name);
typedef void *(*xmlHashCopier)(void *payload, xmlChar *name);
typedef void (*xmlHashScanner)(void *payload, void *data, xmlChar *name);
typedef void (*xmlHashScannerFull)(void *payload, void *data, xmlChar *name, xmlChar *name2, xmlChar *name3);

XMLPUBFUN xmlHashTablePtr xmlHashCreate(int size);
XMLPUBFUN xmlHashTablePtr xmlHashCreateDict(int size, xmlDictPtr dict);
XMLPUBFUN void xmlHashFree(xmlHashTablePtr table, xmlHashDeallocator f);
XMLPUBFUN int xmlHashAddEntry(xmlHashTablePtr table, const xmlChar *name, void *userdata);
XMLPUBFUN int xmlHashAddEntry2(xmlHashTablePtr table, const xmlChar *name,
                                const xmlChar *name2, void *userdata);
XMLPUBFUN int xmlHashAddEntry3(xmlHashTablePtr table, const xmlChar *name,
                                const xmlChar *name2, const xmlChar *name3, void *userdata);
XMLPUBFUN int xmlHashUpdateEntry(xmlHashTablePtr table, const xmlChar *name,
                                  void *userdata, xmlHashDeallocator f);
XMLPUBFUN int xmlHashUpdateEntry2(xmlHashTablePtr table, const xmlChar *name,
                                   const xmlChar *name2, void *userdata, xmlHashDeallocator f);
XMLPUBFUN int xmlHashUpdateEntry3(xmlHashTablePtr table, const xmlChar *name,
                                   const xmlChar *name2, const xmlChar *name3,
                                   void *userdata, xmlHashDeallocator f);
XMLPUBFUN void *xmlHashLookup(xmlHashTablePtr table, const xmlChar *name);
XMLPUBFUN void *xmlHashLookup2(xmlHashTablePtr table, const xmlChar *name,
                                const xmlChar *name2);
XMLPUBFUN void *xmlHashLookup3(xmlHashTablePtr table, const xmlChar *name,
                                const xmlChar *name2, const xmlChar *name3);
XMLPUBFUN int xmlHashSize(xmlHashTablePtr table);
XMLPUBFUN int xmlHashRemoveEntry(xmlHashTablePtr table, const xmlChar *name,
                                  xmlHashDeallocator f);
XMLPUBFUN int xmlHashRemoveEntry2(xmlHashTablePtr table, const xmlChar *name,
                                   const xmlChar *name2, xmlHashDeallocator f);
XMLPUBFUN int xmlHashRemoveEntry3(xmlHashTablePtr table, const xmlChar *name,
                                   const xmlChar *name2, const xmlChar *name3,
                                   xmlHashDeallocator f);
XMLPUBFUN void xmlHashScan(xmlHashTablePtr table, xmlHashScanner f, void *data);
XMLPUBFUN void xmlHashScanFull(xmlHashTablePtr table, xmlHashScannerFull f, void *data);
XMLPUBFUN xmlHashTablePtr xmlHashCopy(xmlHashTablePtr table, xmlHashCopier f);

#ifdef __cplusplus
}
#endif

#endif /* __XML_HASH_H__ */
