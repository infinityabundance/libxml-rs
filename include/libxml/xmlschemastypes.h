/**
 * @file
 *
 * XML Schema types API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_SCHEMASTYPES_H__
#define __XML_SCHEMASTYPES_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlschemas.h>

#ifdef __cplusplus
extern "C" {
#endif

































/* Functions will be declared here as they are implemented. */

















































































































































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef enum{
    XML_SCHEMA_WHITESPACE_UNKNOWN = 0,
    XML_SCHEMA_WHITESPACE_PRESERVE = 1,
    XML_SCHEMA_WHITESPACE_REPLACE = 2,
    XML_SCHEMA_WHITESPACE_COLLAPSE = 3
} xmlSchemaWhitespaceValueType;

/* [11.1-G] end: extracted definitions */

/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN int xmlSchemaCheckFacet (xmlSchemaFacet *facet, xmlSchemaType *typeDecl, xmlSchemaParserCtxt *ctxt, const xmlChar *name);
XMLPUBFUN void xmlSchemaCleanupTypes (void);
XMLPUBFUN xmlChar * xmlSchemaCollapseString (const xmlChar *value);
XMLPUBFUN int xmlSchemaCompareValues (xmlSchemaVal *x, xmlSchemaVal *y);
XMLPUBFUN int xmlSchemaCompareValuesWhtsp (xmlSchemaVal *x, xmlSchemaWhitespaceValueType xws, xmlSchemaVal *y, xmlSchemaWhitespaceValueType yws);
XMLPUBFUN xmlSchemaVal * xmlSchemaCopyValue (xmlSchemaVal *val);
XMLPUBFUN void xmlSchemaFreeFacet (xmlSchemaFacet *facet);
XMLPUBFUN void xmlSchemaFreeValue (xmlSchemaVal *val);
XMLPUBFUN xmlSchemaType * xmlSchemaGetBuiltInListSimpleTypeItemType (xmlSchemaType *type);
XMLPUBFUN xmlSchemaType * xmlSchemaGetBuiltInType (xmlSchemaValType type);
XMLPUBFUN int xmlSchemaGetCanonValue (xmlSchemaVal *val, const xmlChar **retValue);
XMLPUBFUN int xmlSchemaGetCanonValueWhtsp (xmlSchemaVal *val, const xmlChar **retValue, xmlSchemaWhitespaceValueType ws);
XMLPUBFUN unsigned long xmlSchemaGetFacetValueAsULong (xmlSchemaFacet *facet);
XMLPUBFUN xmlSchemaType * xmlSchemaGetPredefinedType (const xmlChar *name, const xmlChar *ns);
XMLPUBFUN xmlSchemaValType xmlSchemaGetValType (xmlSchemaVal *val);
XMLPUBFUN int xmlSchemaInitTypes (void);
XMLPUBFUN int xmlSchemaIsBuiltInTypeFacet (xmlSchemaType *type, int facetType);
XMLPUBFUN xmlSchemaFacet * xmlSchemaNewFacet (void);
XMLPUBFUN xmlSchemaVal * xmlSchemaNewNOTATIONValue (const xmlChar *name, const xmlChar *ns);
XMLPUBFUN xmlSchemaVal * xmlSchemaNewQNameValue (const xmlChar *namespaceName, const xmlChar *localName);
XMLPUBFUN xmlSchemaVal * xmlSchemaNewStringValue (xmlSchemaValType type, const xmlChar *value);
XMLPUBFUN int xmlSchemaValPredefTypeNode (xmlSchemaType *type, const xmlChar *value, xmlSchemaVal **val, xmlNode *node);
XMLPUBFUN int xmlSchemaValPredefTypeNodeNoNorm(xmlSchemaType *type, const xmlChar *value, xmlSchemaVal **val, xmlNode *node);
XMLPUBFUN int xmlSchemaValidateFacet (xmlSchemaType *base, xmlSchemaFacet *facet, const xmlChar *value, xmlSchemaVal *val);
XMLPUBFUN int xmlSchemaValidateFacetWhtsp (xmlSchemaFacet *facet, xmlSchemaWhitespaceValueType fws, xmlSchemaValType valType, const xmlChar *value, xmlSchemaVal *val, xmlSchemaWhitespaceValueType ws);
XMLPUBFUN int xmlSchemaValidateLengthFacet (xmlSchemaType *type, xmlSchemaFacet *facet, const xmlChar *value, xmlSchemaVal *val, unsigned long *length);
XMLPUBFUN int xmlSchemaValidateLengthFacetWhtsp(xmlSchemaFacet *facet, xmlSchemaValType valType, const xmlChar *value, xmlSchemaVal *val, unsigned long *length, xmlSchemaWhitespaceValueType ws);
XMLPUBFUN int xmlSchemaValidateListSimpleTypeFacet (xmlSchemaFacet *facet, const xmlChar *value, unsigned long actualLen, unsigned long *expectedLen);
XMLPUBFUN int xmlSchemaValidatePredefinedType (xmlSchemaType *type, const xmlChar *value, xmlSchemaVal **val);
XMLPUBFUN int xmlSchemaValueAppend (xmlSchemaVal *prev, xmlSchemaVal *cur);
XMLPUBFUN int xmlSchemaValueGetAsBoolean (xmlSchemaVal *val);
XMLPUBFUN const xmlChar * xmlSchemaValueGetAsString (xmlSchemaVal *val);
XMLPUBFUN xmlSchemaVal * xmlSchemaValueGetNext (xmlSchemaVal *cur);
XMLPUBFUN xmlChar * xmlSchemaWhiteSpaceReplace (const xmlChar *value);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_SCHEMASTYPES_H__ */
