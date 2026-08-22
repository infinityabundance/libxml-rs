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

XMLPUBFUN int xmlInitThreads(void);
XMLPUBFUN void xmlCleanupThreads(void);
XMLPUBFUN void xmlLockLibrary(void);
XMLPUBFUN void xmlUnlockLibrary(void);

#ifdef __cplusplus
}
#endif

#endif /* __XML_THREADS_H__ */
