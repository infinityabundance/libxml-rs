/**
 * @file
 *
 * Hash table API for libxml-rs
 */

#ifndef __XML_HASH_H__
#define __XML_HASH_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>
#include <libxml/dict.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlHashTable xmlHashTable;
typedef xmlHashTable *xmlHashTablePtr;

typedef void (*xmlHashDeallocator)(void *payload, const xmlChar *name);
typedef void *(*xmlHashCopier)(void *payload, const xmlChar *name);
typedef void (*xmlHashScanner)(void *payload, void *data, const xmlChar *name);
typedef void (*xmlHashScannerFull)(void *payload, void *data, const xmlChar *name,
				  const xmlChar *name2, const xmlChar *name3);

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


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN int xmlHashAdd (xmlHashTable *hash, const xmlChar *name, void *userdata);
XMLPUBFUN int xmlHashAdd2 (xmlHashTable *hash, const xmlChar *name, const xmlChar *name2, void *userdata);
XMLPUBFUN int xmlHashAdd3 (xmlHashTable *hash, const xmlChar *name, const xmlChar *name2, const xmlChar *name3, void *userdata);
XMLPUBFUN xmlHashTable * xmlHashCopySafe (xmlHashTable *hash, xmlHashCopier copy, xmlHashDeallocator dealloc);
XMLPUBFUN void xmlHashDefaultDeallocator(void *entry, const xmlChar *name);
XMLPUBFUN void * xmlHashQLookup (xmlHashTable *hash, const xmlChar *prefix, const xmlChar *name);
XMLPUBFUN void * xmlHashQLookup2 (xmlHashTable *hash, const xmlChar *prefix, const xmlChar *name, const xmlChar *prefix2, const xmlChar *name2);
XMLPUBFUN void * xmlHashQLookup3 (xmlHashTable *hash, const xmlChar *prefix, const xmlChar *name, const xmlChar *prefix2, const xmlChar *name2, const xmlChar *prefix3, const xmlChar *name3);
XMLPUBFUN void xmlHashScan3 (xmlHashTable *hash, const xmlChar *name, const xmlChar *name2, const xmlChar *name3, xmlHashScanner scan, void *data);
XMLPUBFUN void xmlHashScanFull3 (xmlHashTable *hash, const xmlChar *name, const xmlChar *name2, const xmlChar *name3, xmlHashScannerFull scan, void *data);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_HASH_H__ */
