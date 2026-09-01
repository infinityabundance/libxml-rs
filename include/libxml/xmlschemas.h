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

/* Validation callback types (upstream xmlschemas.h) */
typedef void (*xmlSchemaValidityErrorFunc) (void *ctx, const char *msg, ...) LIBXML_ATTR_FORMAT(2,3);
typedef void (*xmlSchemaValidityWarningFunc) (void *ctx, const char *msg, ...) LIBXML_ATTR_FORMAT(2,3);
typedef int (*xmlSchemaValidityLocatorFunc) (void *ctx,
                           const char **file, unsigned long *line);

































/* Functions will be declared here as they are implemented. */

















































































































































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlSchema xmlSchema;
typedef xmlSchema *xmlSchemaPtr;
typedef struct _xmlSchemaSAXPlug xmlSchemaSAXPlugStruct;
typedef xmlSchemaSAXPlugStruct *xmlSchemaSAXPlugPtr;

typedef struct _xmlSchemaParserCtxt xmlSchemaParserCtxt;
typedef xmlSchemaParserCtxt *xmlSchemaParserCtxtPtr;

typedef struct _xmlSchemaValidCtxt xmlSchemaValidCtxt;
typedef xmlSchemaValidCtxt *xmlSchemaValidCtxtPtr;

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

/* [11.1-G] end: extracted definitions */

/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN void xmlSchemaDump (FILE *output, xmlSchema *schema);
XMLPUBFUN void xmlSchemaFree (xmlSchema *schema);
XMLPUBFUN void xmlSchemaFreeParserCtxt (xmlSchemaParserCtxt *ctxt);
XMLPUBFUN void xmlSchemaFreeValidCtxt (xmlSchemaValidCtxt *ctxt);
XMLPUBFUN int xmlSchemaGetParserErrors (xmlSchemaParserCtxt *ctxt, xmlSchemaValidityErrorFunc * err, xmlSchemaValidityWarningFunc * warn, void **ctx);
XMLPUBFUN int xmlSchemaGetValidErrors (xmlSchemaValidCtxt *ctxt, xmlSchemaValidityErrorFunc *err, xmlSchemaValidityWarningFunc *warn, void **ctx);
XMLPUBFUN int xmlSchemaIsValid (xmlSchemaValidCtxt *ctxt);
XMLPUBFUN xmlSchemaParserCtxt * xmlSchemaNewDocParserCtxt (xmlDoc *doc);
XMLPUBFUN xmlSchemaParserCtxt * xmlSchemaNewMemParserCtxt (const char *buffer, int size);
XMLPUBFUN xmlSchemaParserCtxt * xmlSchemaNewParserCtxt (const char *URL);
XMLPUBFUN xmlSchemaValidCtxt * xmlSchemaNewValidCtxt (xmlSchema *schema);
XMLPUBFUN xmlSchema * xmlSchemaParse (xmlSchemaParserCtxt *ctxt);
XMLPUBFUN xmlSchemaSAXPlugStruct * xmlSchemaSAXPlug (xmlSchemaValidCtxt *ctxt, xmlSAXHandler **sax, void **user_data);
XMLPUBFUN int xmlSchemaSAXUnplug (xmlSchemaSAXPlugStruct *plug);
XMLPUBFUN void xmlSchemaSetParserErrors (xmlSchemaParserCtxt *ctxt, xmlSchemaValidityErrorFunc err, xmlSchemaValidityWarningFunc warn, void *ctx);
XMLPUBFUN void xmlSchemaSetParserStructuredErrors(xmlSchemaParserCtxt *ctxt, xmlStructuredErrorFunc serror, void *ctx);
XMLPUBFUN void xmlSchemaSetResourceLoader (xmlSchemaParserCtxt *ctxt, xmlResourceLoader loader, void *data);
XMLPUBFUN void xmlSchemaSetValidErrors (xmlSchemaValidCtxt *ctxt, xmlSchemaValidityErrorFunc err, xmlSchemaValidityWarningFunc warn, void *ctx);
XMLPUBFUN int xmlSchemaSetValidOptions (xmlSchemaValidCtxt *ctxt, int options);
XMLPUBFUN void xmlSchemaSetValidStructuredErrors(xmlSchemaValidCtxt *ctxt, xmlStructuredErrorFunc serror, void *ctx);
XMLPUBFUN int xmlSchemaValidCtxtGetOptions(xmlSchemaValidCtxt *ctxt);
XMLPUBFUN xmlParserCtxt * xmlSchemaValidCtxtGetParserCtxt(xmlSchemaValidCtxt *ctxt);
XMLPUBFUN int xmlSchemaValidateDoc (xmlSchemaValidCtxt *ctxt, xmlDoc *instance);
XMLPUBFUN int xmlSchemaValidateFile (xmlSchemaValidCtxt *ctxt, const char * filename, int options);
XMLPUBFUN int xmlSchemaValidateOneElement (xmlSchemaValidCtxt *ctxt, xmlNode *elem);
XMLPUBFUN void xmlSchemaValidateSetFilename(xmlSchemaValidCtxt *vctxt, const char *filename);
XMLPUBFUN void xmlSchemaValidateSetLocator (xmlSchemaValidCtxt *vctxt, xmlSchemaValidityLocatorFunc f, void *ctxt);
XMLPUBFUN int xmlSchemaValidateStream (xmlSchemaValidCtxt *ctxt, xmlParserInputBuffer *input, xmlCharEncoding enc, const xmlSAXHandler *sax, void *user_data);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_SCHEMAS_H__ */
