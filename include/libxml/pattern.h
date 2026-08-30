/**
 * @file
 *
 * Pattern API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_PATTERN_H__
#define __XML_PATTERN_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

































/* Functions will be declared here as they are implemented. */

















































































































































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlPattern xmlPattern;

typedef struct _xmlStreamCtxt xmlStreamCtxt;

typedef enum{
    XML_PATTERN_DEFAULT		= 0,	/* simple pattern match */
    XML_PATTERN_XPATH		= 1<<0,	/* standard XPath pattern */
    XML_PATTERN_XSSEL		= 1<<1,	/* XPath subset for schema selector */
    XML_PATTERN_XSFIELD		= 1<<2	/* XPath subset for schema field */
} xmlPatternFlags;

/* [11.1-G] end: extracted definitions */
#ifdef __cplusplus
}
#endif

#endif /* __XML_PATTERN_H__ */
