/**
 * @file
 *
 * XSLT document API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __DOCUMENTS_H__
#define __DOCUMENTS_H__

#include <libxml/xmlversion.h>
#include <libxslt/xslt.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Functions will be declared here as they are implemented. */










































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef enum{
    XSLT_LOAD_START = 0,	/* loading for a top stylesheet */
    XSLT_LOAD_STYLESHEET = 1,	/* loading for a stylesheet include/import */
    XSLT_LOAD_DOCUMENT = 2	/* loading document at transformation time */
} xsltLoadType;

/* [11.1-G] end: extracted definitions */
#ifdef __cplusplus
}
#endif

#endif /* __DOCUMENTS_H__ */
