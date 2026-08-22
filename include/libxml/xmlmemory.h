/**
 * @file
 *
 * Memory allocator interface for libxml-rs
 */

#ifndef __DEBUG_MEMORY_ALLOC__
#define __DEBUG_MEMORY_ALLOC__

#include <stdio.h>
#include <stdlib.h>
#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void (*xmlFreeFunc)(void *mem);
typedef void *(*xmlMallocFunc)(size_t size);
typedef void *(*xmlReallocFunc)(void *mem, size_t size);
typedef char *(*xmlStrdupFunc)(const char *str);

XMLPUBVAR xmlMallocFunc xmlMalloc;
XMLPUBVAR xmlMallocFunc xmlMallocAtomic;
XMLPUBVAR xmlReallocFunc xmlRealloc;
XMLPUBVAR xmlFreeFunc xmlFree;
XMLPUBVAR xmlStrdupFunc xmlMemStrdup;

XMLPUBFUN int xmlMemSetup(xmlFreeFunc freeFunc,
                           xmlMallocFunc mallocFunc,
                           xmlReallocFunc reallocFunc,
                           xmlStrdupFunc strdupFunc);
XMLPUBFUN int xmlMemGet(xmlFreeFunc *freeFunc,
                         xmlMallocFunc *mallocFunc,
                         xmlReallocFunc *reallocFunc,
                         xmlStrdupFunc *strdupFunc);
XMLPUBFUN int xmlGcMemSetup(xmlFreeFunc freeFunc,
                             xmlMallocFunc mallocFunc,
                             xmlMallocFunc mallocAtomicFunc,
                             xmlReallocFunc reallocFunc,
                             xmlStrdupFunc strdupFunc);
XMLPUBFUN int xmlGcMemGet(xmlFreeFunc *freeFunc,
                           xmlMallocFunc *mallocFunc,
                           xmlMallocFunc *mallocAtomicFunc,
                           xmlReallocFunc *reallocFunc,
                           xmlStrdupFunc *strdupFunc);
XMLPUBFUN int xmlInitMemory(void);
XMLPUBFUN void xmlCleanupMemory(void);
XMLPUBFUN size_t xmlMemSize(void *ptr);
XMLPUBFUN int xmlMemUsed(void);
XMLPUBFUN int xmlMemBlocks(void);
XMLPUBFUN void xmlMemDisplay(FILE *fp);
XMLPUBFUN void xmlMemDisplayLast(FILE *fp, long nbBytes);
XMLPUBFUN void xmlMemShow(FILE *fp, int nr);
XMLPUBFUN void xmlMemoryDump(void);
XMLPUBFUN void *xmlMemMalloc(size_t size);
XMLPUBFUN void *xmlMemRealloc(void *ptr, size_t size);
XMLPUBFUN void xmlMemFree(void *ptr);
XMLPUBFUN char *xmlMemoryStrdup(const char *str);
XMLPUBFUN void *xmlMallocLoc(size_t size, const char *file, int line);
XMLPUBFUN void *xmlReallocLoc(void *ptr, size_t size, const char *file, int line);
XMLPUBFUN void *xmlMallocAtomicLoc(size_t size, const char *file, int line);
XMLPUBFUN char *xmlMemStrdupLoc(const char *str, const char *file, int line);

#ifdef __cplusplus
}
#endif

#endif /* __DEBUG_MEMORY_ALLOC__ */
