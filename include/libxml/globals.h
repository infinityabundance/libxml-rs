/**
 * @file
 *
 * Global state API for libxml-rs
 */

#ifndef __XML_GLOBALS_H__
#define __XML_GLOBALS_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlerror.h>
#include <libxml/parser.h>

#ifdef __cplusplus
extern "C" {
#endif

XMLPUBFUN void xmlInitGlobals(void);
XMLPUBFUN void xmlCleanupGlobals(void);

#ifdef __cplusplus
}
#endif

#endif /* __XML_GLOBALS_H__ */
