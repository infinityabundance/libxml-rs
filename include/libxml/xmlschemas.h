/**
 * @file
 *
 * XML Schema API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_SCHEMAS_H__
#define __XML_SCHEMAS_H__

#include <libxml/xmlversion.h>
#include <libxml/encoding.h>
#include <libxml/parser.h>
#include <libxml/tree.h>
#include <libxml/schemasInternals.h>

#ifdef __cplusplus
extern "C" {
#endif

































/* Functions will be declared here as they are implemented. */

















































































































































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlSchema xmlSchema;
typedef xmlSchema *xmlSchemaPtr;
typedef struct _xmlSchemaSAXPlug xmlSchemaSAXPlugStruct;
typedef xmlSchemaSAXPlugStruct *xmlSchemaSAXPlugPtr;

typedef struct _xmlSchemaParserCtxt xmlSchemaParserCtxt;

typedef struct _xmlSchemaValidCtxt xmlSchemaValidCtxt;

typedef enum{
    XML_SCHEMAS_ERR_OK		= 0,
    XML_SCHEMAS_ERR_NOROOT	= 1,
    XML_SCHEMAS_ERR_UNDECLAREDELEM,
    XML_SCHEMAS_ERR_NOTTOPLEVEL,
    XML_SCHEMAS_ERR_MISSING,
    XML_SCHEMAS_ERR_WRONGELEM,
    XML_SCHEMAS_ERR_NOTYPE,
    XML_SCHEMAS_ERR_NOROLLBACK,
    XML_SCHEMAS_ERR_ISABSTRACT,
    XML_SCHEMAS_ERR_NOTEMPTY,
    XML_SCHEMAS_ERR_ELEMCONT,
    XML_SCHEMAS_ERR_HAVEDEFAULT,
    XML_SCHEMAS_ERR_NOTNILLABLE,
    XML_SCHEMAS_ERR_EXTRACONTENT,
    XML_SCHEMAS_ERR_INVALIDATTR,
    XML_SCHEMAS_ERR_INVALIDELEM,
    XML_SCHEMAS_ERR_NOTDETERMINIST,
    XML_SCHEMAS_ERR_CONSTRUCT,
    XML_SCHEMAS_ERR_INTERNAL,
    XML_SCHEMAS_ERR_NOTSIMPLE,
    XML_SCHEMAS_ERR_ATTRUNKNOWN,
    XML_SCHEMAS_ERR_ATTRINVALID,
    XML_SCHEMAS_ERR_VALUE,
    XML_SCHEMAS_ERR_FACET,
    XML_SCHEMAS_ERR_,
    XML_SCHEMAS_ERR_XXX
} xmlSchemaValidError;

typedef enum{
    XML_SCHEMA_VAL_VC_I_CREATE			= 1<<0
} xmlSchemaValidOption;

typedef xmlSchemaValidCtxt *xmlSchemaValidCtxtPtr;

/* [11.1-G] end: extracted definitions */
#ifdef __cplusplus
}
#endif

#endif /* __XML_SCHEMAS_H__ */
