/**
 * @file
 *
 * Save/serialization API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_XMLSAVE_H__
#define __XML_XMLSAVE_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

































/* Functions will be declared here as they are implemented. */









































































































































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlSaveCtxt xmlSaveCtxt;

typedef enum{
    XML_SAVE_FORMAT     = 1<<0,
    XML_SAVE_NO_DECL    = 1<<1,
    XML_SAVE_NO_EMPTY	= 1<<2,
    XML_SAVE_NO_XHTML	= 1<<3,
    XML_SAVE_XHTML	= 1<<4,
    XML_SAVE_AS_XML     = 1<<5,
    XML_SAVE_AS_HTML    = 1<<6,
    XML_SAVE_WSNONSIG   = 1<<7,
    XML_SAVE_EMPTY      = 1<<8,
    XML_SAVE_NO_INDENT  = 1<<9,
    XML_SAVE_INDENT     = 1<<10
} xmlSaveOption;

/* [11.1-G] end: extracted definitions */
#ifdef __cplusplus
}
#endif

#endif /* __XML_XMLSAVE_H__ */
