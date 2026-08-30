/**
 * @file
 *
 * Thread support API for libxml-rs
 */

#ifndef __XML_THREADS_H__
#define __XML_THREADS_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Mutex types (upstream threads.h) */
typedef struct _xmlMutex xmlMutex;
typedef xmlMutex *xmlMutexPtr;
typedef struct _xmlRMutex xmlRMutex;
typedef xmlRMutex *xmlRMutexPtr;
XMLPUBFUN int xmlInitThreads(void);
XMLPUBFUN void xmlCleanupThreads(void);
XMLPUBFUN void xmlLockLibrary(void);
XMLPUBFUN void xmlUnlockLibrary(void);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN int xmlCheckThreadLocalStorage(void);
XMLPUBFUN void xmlFreeMutex (xmlMutex *tok);
XMLPUBFUN void xmlFreeRMutex (xmlRMutex *tok);
XMLPUBFUN void xmlMutexLock (xmlMutex *tok);
XMLPUBFUN void xmlMutexUnlock (xmlMutex *tok);
XMLPUBFUN xmlMutex * xmlNewMutex (void);
XMLPUBFUN xmlRMutex * xmlNewRMutex (void);
XMLPUBFUN void xmlRMutexLock (xmlRMutex *tok);
XMLPUBFUN void xmlRMutexUnlock (xmlRMutex *tok);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_THREADS_H__ */
