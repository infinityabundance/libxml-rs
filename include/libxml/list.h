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

typedef struct _xmlList xmlList;
typedef xmlList *xmlListPtr;

typedef void (*xmlListDeallocator)(void *data);
typedef int (*xmlListDataCompare)(const void *data1, const void *data2);
typedef int (*xmlListWalker)(void *data, void *user_data);

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
XMLPUBFUN void *xmlListFront(xmlListPtr list);
XMLPUBFUN void *xmlListBack(xmlListPtr list);
XMLPUBFUN int xmlListSize(xmlListPtr list);
XMLPUBFUN void xmlListSort(xmlListPtr list);
XMLPUBFUN void xmlListReverse(xmlListPtr list);
XMLPUBFUN void xmlListReverseSplice(xmlListPtr list, xmlListPtr list2);
XMLPUBFUN void xmlListMerge(xmlListPtr list, xmlListPtr list2);

#ifdef __cplusplus
}
#endif

#endif /* __XML_LIST_H__ */
