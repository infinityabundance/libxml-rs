/**
 * @file
 *
 * Linked list API for libxml-rs
 */

#ifndef __XML_LIST_H__
#define __XML_LIST_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlLink xmlLink;
typedef struct _xmlList xmlList;
typedef xmlList *xmlListPtr;

typedef void (*xmlListDeallocator)(xmlLink *lk);
typedef int (*xmlListDataCompare)(const void *data0, const void *data1);
typedef int (*xmlListWalker)(const void *data, void *user);

XMLPUBFUN xmlListPtr xmlListCreate(xmlListDeallocator deallocator,
                                    xmlListDataCompare compare);
XMLPUBFUN void xmlListDelete(xmlListPtr list);
XMLPUBFUN void *xmlListSearch(xmlListPtr list, void *data);
XMLPUBFUN void xmlListWalk(xmlListPtr list, xmlListWalker walker, void *data);
XMLPUBFUN int xmlListPushBack(xmlListPtr list, void *data);
XMLPUBFUN int xmlListPushFront(xmlListPtr list, void *data);
XMLPUBFUN void xmlListPopBack(xmlListPtr list);
XMLPUBFUN void xmlListPopFront(xmlListPtr list);
XMLPUBFUN int xmlListInsert(xmlListPtr list, void *data);
XMLPUBFUN int xmlListAppend(xmlListPtr list, void *data);
XMLPUBFUN int xmlListRemoveFirst(xmlListPtr list, void *data);
XMLPUBFUN int xmlListRemoveLast(xmlListPtr list, void *data);
XMLPUBFUN int xmlListRemoveAll(xmlListPtr list, void *data);
XMLPUBFUN void xmlListClear(xmlListPtr list);
XMLPUBFUN int xmlListEmpty(xmlListPtr list);
XMLPUBFUN xmlLink *xmlListFront(xmlListPtr list);
XMLPUBFUN xmlLink *xmlListBack(xmlListPtr list);
XMLPUBFUN int xmlListSize(xmlListPtr list);
XMLPUBFUN void xmlListSort(xmlListPtr list);
XMLPUBFUN void xmlListReverse(xmlListPtr list);
XMLPUBFUN void xmlListReverseSplice(xmlListPtr list, xmlListPtr list2);
XMLPUBFUN void xmlListMerge(xmlListPtr list, xmlListPtr list2);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN void * xmlLinkGetData (xmlLink *lk);
XMLPUBFUN int xmlListCopy (xmlList *cur, xmlList *old);
XMLPUBFUN xmlList * xmlListDup (xmlList *old);
XMLPUBFUN xmlLink * xmlListEnd (xmlList *l);
XMLPUBFUN void * xmlListReverseSearch (xmlList *l, void *data);
XMLPUBFUN void xmlListReverseWalk (xmlList *l, xmlListWalker walker, void *user);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_LIST_H__ */
