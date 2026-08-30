/**
 * @file
 *
 * Schema internals for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XML_SCHEMAS_INTERNALS_H__
#define __XML_SCHEMAS_INTERNALS_H__

#include <libxml/xmlversion.h>
#include <libxml/xmlregexp.h>
#include <libxml/hash.h>
#include <libxml/dict.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

























































































































































/* Functions will be declared here as they are implemented. */



































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlSchemaType xmlSchemaType;
typedef xmlSchemaType *xmlSchemaTypePtr;
typedef struct _xmlSchemaFacet xmlSchemaFacet;
typedef xmlSchemaFacet *xmlSchemaFacetPtr;

typedef struct _xmlSchemaAnnot xmlSchemaAnnot;
typedef xmlSchemaAnnot *xmlSchemaAnnotPtr;
/**
 * Annotation
 */

typedef struct _xmlSchemaAttribute xmlSchemaAttribute;
typedef xmlSchemaAttribute *xmlSchemaAttributePtr;
/**
 * An attribute definition.
 */

typedef struct _xmlSchemaAttributeGroup xmlSchemaAttributeGroup;
typedef xmlSchemaAttributeGroup *xmlSchemaAttributeGroupPtr;
/**
 * An attribute group definition.
 *
 * xmlSchemaAttribute and xmlSchemaAttributeGroup start of structures
 * must be kept similar
 */

typedef struct _xmlSchemaAttributeLink xmlSchemaAttributeLink;
typedef xmlSchemaAttributeLink *xmlSchemaAttributeLinkPtr;
/**
 * Used to build a list of attribute uses on complexType definitions.
 * WARNING: Deprecated; not used.
 */

typedef struct _xmlSchemaElement xmlSchemaElement;
typedef xmlSchemaElement *xmlSchemaElementPtr;
/**
 * An element definition.
 *
 * xmlSchemaType, xmlSchemaFacet and xmlSchemaElement start of
 * structures must be kept similar
 */

typedef struct _xmlSchemaFacetLink xmlSchemaFacetLink;
typedef xmlSchemaFacetLink *xmlSchemaFacetLinkPtr;
/**
 * Used to build a list of facets.
 */

typedef struct _xmlSchemaNotation xmlSchemaNotation;
typedef xmlSchemaNotation *xmlSchemaNotationPtr;
/**
 * A notation definition.
 */

typedef struct _xmlSchemaTypeLink xmlSchemaTypeLink;
typedef xmlSchemaTypeLink *xmlSchemaTypeLinkPtr;
/**
 * Used to build a list of types (e.g. member types of
 * simpleType with variety "union").
 */

typedef struct _xmlSchemaVal xmlSchemaVal;

typedef struct _xmlSchemaWildcard xmlSchemaWildcard;
typedef xmlSchemaWildcard *xmlSchemaWildcardPtr;
/**
 * A wildcard.
 */

typedef struct _xmlSchemaWildcardNs xmlSchemaWildcardNs;
typedef xmlSchemaWildcardNs *xmlSchemaWildcardNsPtr;
/**
 * Used to build a list of namespaces on wildcards.
 */

typedef enum{
    XML_SCHEMAS_UNKNOWN = 0,
    XML_SCHEMAS_STRING = 1,
    XML_SCHEMAS_NORMSTRING = 2,
    XML_SCHEMAS_DECIMAL = 3,
    XML_SCHEMAS_TIME = 4,
    XML_SCHEMAS_GDAY = 5,
    XML_SCHEMAS_GMONTH = 6,
    XML_SCHEMAS_GMONTHDAY = 7,
    XML_SCHEMAS_GYEAR = 8,
    XML_SCHEMAS_GYEARMONTH = 9,
    XML_SCHEMAS_DATE = 10,
    XML_SCHEMAS_DATETIME = 11,
    XML_SCHEMAS_DURATION = 12,
    XML_SCHEMAS_FLOAT = 13,
    XML_SCHEMAS_DOUBLE = 14,
    XML_SCHEMAS_BOOLEAN = 15,
    XML_SCHEMAS_TOKEN = 16,
    XML_SCHEMAS_LANGUAGE = 17,
    XML_SCHEMAS_NMTOKEN = 18,
    XML_SCHEMAS_NMTOKENS = 19,
    XML_SCHEMAS_NAME = 20,
    XML_SCHEMAS_QNAME = 21,
    XML_SCHEMAS_NCNAME = 22,
    XML_SCHEMAS_ID = 23,
    XML_SCHEMAS_IDREF = 24,
    XML_SCHEMAS_IDREFS = 25,
    XML_SCHEMAS_ENTITY = 26,
    XML_SCHEMAS_ENTITIES = 27,
    XML_SCHEMAS_NOTATION = 28,
    XML_SCHEMAS_ANYURI = 29,
    XML_SCHEMAS_INTEGER = 30,
    XML_SCHEMAS_NPINTEGER = 31,
    XML_SCHEMAS_NINTEGER = 32,
    XML_SCHEMAS_NNINTEGER = 33,
    XML_SCHEMAS_PINTEGER = 34,
    XML_SCHEMAS_INT = 35,
    XML_SCHEMAS_UINT = 36,
    XML_SCHEMAS_LONG = 37,
    XML_SCHEMAS_ULONG = 38,
    XML_SCHEMAS_SHORT = 39,
    XML_SCHEMAS_USHORT = 40,
    XML_SCHEMAS_BYTE = 41,
    XML_SCHEMAS_UBYTE = 42,
    XML_SCHEMAS_HEXBINARY = 43,
    XML_SCHEMAS_BASE64BINARY = 44,
    XML_SCHEMAS_ANYTYPE = 45,
    XML_SCHEMAS_ANYSIMPLETYPE = 46
} xmlSchemaValType;

typedef enum{
    XML_SCHEMA_CONTENT_UNKNOWN = 0,
    XML_SCHEMA_CONTENT_EMPTY = 1,
    XML_SCHEMA_CONTENT_ELEMENTS,
    XML_SCHEMA_CONTENT_MIXED,
    XML_SCHEMA_CONTENT_SIMPLE,
    XML_SCHEMA_CONTENT_MIXED_OR_ELEMENTS, /* Obsolete */
    XML_SCHEMA_CONTENT_BASIC,
    XML_SCHEMA_CONTENT_ANY
} xmlSchemaContentType;

typedef enum{
    XML_SCHEMA_TYPE_BASIC = 1, /* A built-in datatype */
    XML_SCHEMA_TYPE_ANY,
    XML_SCHEMA_TYPE_FACET,
    XML_SCHEMA_TYPE_SIMPLE,
    XML_SCHEMA_TYPE_COMPLEX,
    XML_SCHEMA_TYPE_SEQUENCE = 6,
    XML_SCHEMA_TYPE_CHOICE,
    XML_SCHEMA_TYPE_ALL,
    XML_SCHEMA_TYPE_SIMPLE_CONTENT,
    XML_SCHEMA_TYPE_COMPLEX_CONTENT,
    XML_SCHEMA_TYPE_UR,
    XML_SCHEMA_TYPE_RESTRICTION,
    XML_SCHEMA_TYPE_EXTENSION,
    XML_SCHEMA_TYPE_ELEMENT,
    XML_SCHEMA_TYPE_ATTRIBUTE,
    XML_SCHEMA_TYPE_ATTRIBUTEGROUP,
    XML_SCHEMA_TYPE_GROUP,
    XML_SCHEMA_TYPE_NOTATION,
    XML_SCHEMA_TYPE_LIST,
    XML_SCHEMA_TYPE_UNION,
    XML_SCHEMA_TYPE_ANY_ATTRIBUTE,
    XML_SCHEMA_TYPE_IDC_UNIQUE,
    XML_SCHEMA_TYPE_IDC_KEY,
    XML_SCHEMA_TYPE_IDC_KEYREF,
    XML_SCHEMA_TYPE_PARTICLE = 25,
    XML_SCHEMA_TYPE_ATTRIBUTE_USE,
    XML_SCHEMA_FACET_MININCLUSIVE = 1000,
    XML_SCHEMA_FACET_MINEXCLUSIVE,
    XML_SCHEMA_FACET_MAXINCLUSIVE,
    XML_SCHEMA_FACET_MAXEXCLUSIVE,
    XML_SCHEMA_FACET_TOTALDIGITS,
    XML_SCHEMA_FACET_FRACTIONDIGITS,
    XML_SCHEMA_FACET_PATTERN,
    XML_SCHEMA_FACET_ENUMERATION,
    XML_SCHEMA_FACET_WHITESPACE,
    XML_SCHEMA_FACET_LENGTH,
    XML_SCHEMA_FACET_MAXLENGTH,
    XML_SCHEMA_FACET_MINLENGTH,
    XML_SCHEMA_EXTRA_QNAMEREF = 2000,
    XML_SCHEMA_EXTRA_ATTR_USE_PROHIB
} xmlSchemaTypeType;

struct _xmlSchema {
    const xmlChar *name; /* schema name */
    const xmlChar *targetNamespace; /* the target namespace */
    const xmlChar *version;
    const xmlChar *id; /* Obsolete */
    xmlDoc *doc;
    xmlSchemaAnnot *annot;
    int flags;

    xmlHashTable *typeDecl;
    xmlHashTable *attrDecl;
    xmlHashTable *attrgrpDecl;
    xmlHashTable *elemDecl;
    xmlHashTable *notaDecl;

    xmlHashTable *schemasImports;

    void *_private;        /* unused by the library for users or bindings */
    xmlHashTable *groupDecl;
    xmlDict        *dict;
    void *includes;     /* the includes, this is opaque for now */
    int preserve;        /* whether to free the document */
    int counter; /* used to give anonymous components unique names */
    xmlHashTable *idcDef; /* All identity-constraint defs. */
    void *volatiles; /* Obsolete */
};

struct _xmlSchemaAnnot {
    struct _xmlSchemaAnnot *next;
    xmlNode *content;         /* the annotation */
};

struct _xmlSchemaAttribute {
    xmlSchemaTypeType type;
    struct _xmlSchemaAttribute *next; /* the next attribute (not used?) */
    const xmlChar *name; /* the name of the declaration */
    const xmlChar *id; /* Deprecated; not used */
    const xmlChar *ref; /* Deprecated; not used */
    const xmlChar *refNs; /* Deprecated; not used */
    const xmlChar *typeName; /* the local name of the type definition */
    const xmlChar *typeNs; /* the ns URI of the type definition */
    xmlSchemaAnnot *annot;

    xmlSchemaType *base; /* Deprecated; not used */
    int occurs; /* Deprecated; not used */
    const xmlChar *defValue; /* The initial value of the value constraint */
    xmlSchemaType *subtypes; /* the type definition */
    xmlNode *node;
    const xmlChar *targetNamespace;
    int flags;
    const xmlChar *refPrefix; /* Deprecated; not used */
    xmlSchemaVal *defVal; /* The compiled value constraint */
    xmlSchemaAttribute *refDecl; /* Deprecated; not used */
};

struct _xmlSchemaAttributeGroup {
    xmlSchemaTypeType type;        /* The kind of type */
    struct _xmlSchemaAttribute *next;/* the next attribute if in a group ... */
    const xmlChar *name;
    const xmlChar *id;
    const xmlChar *ref; /* Deprecated; not used */
    const xmlChar *refNs; /* Deprecated; not used */
    xmlSchemaAnnot *annot;

    xmlSchemaAttribute *attributes; /* Deprecated; not used */
    xmlNode *node;
    int flags;
    xmlSchemaWildcard *attributeWildcard;
    const xmlChar *refPrefix; /* Deprecated; not used */
    xmlSchemaAttributeGroup *refItem; /* Deprecated; not used */
    const xmlChar *targetNamespace;
    void *attrUses;
};

struct _xmlSchemaAttributeLink {
    struct _xmlSchemaAttributeLink *next;/* the next attribute link ... */
    struct _xmlSchemaAttribute *attr;/* the linked attribute */
};

struct _xmlSchemaElement {
    xmlSchemaTypeType type; /* The kind of type */
    struct _xmlSchemaType *next; /* Not used? */
    const xmlChar *name;
    const xmlChar *id; /* Deprecated; not used */
    const xmlChar *ref; /* Deprecated; not used */
    const xmlChar *refNs; /* Deprecated; not used */
    xmlSchemaAnnot *annot;
    xmlSchemaType *subtypes; /* the type definition */
    xmlSchemaAttribute *attributes;
    xmlNode *node;
    int minOccurs; /* Deprecated; not used */
    int maxOccurs; /* Deprecated; not used */

    int flags;
    const xmlChar *targetNamespace;
    const xmlChar *namedType;
    const xmlChar *namedTypeNs;
    const xmlChar *substGroup;
    const xmlChar *substGroupNs;
    const xmlChar *scope;
    const xmlChar *value; /* The original value of the value constraint. */
    struct _xmlSchemaElement *refDecl; /* This will now be used for the
                                          substitution group affiliation */
    xmlRegexp *contModel; /* Obsolete for WXS, maybe used for RelaxNG */
    xmlSchemaContentType contentType;
    const xmlChar *refPrefix; /* Deprecated; not used */
    xmlSchemaVal *defVal; /* The compiled value constraint. */
    void *idcs; /* The identity-constraint defs */
};

struct _xmlSchemaFacet {
    xmlSchemaTypeType type;        /* The kind of type */
    struct _xmlSchemaFacet *next;/* the next type if in a sequence ... */
    const xmlChar *value; /* The original value */
    const xmlChar *id; /* Obsolete */
    xmlSchemaAnnot *annot;
    xmlNode *node;
    int fixed; /* XML_SCHEMAS_FACET_PRESERVE, etc. */
    int whitespace;
    xmlSchemaVal *val; /* The compiled value */
    xmlRegexp      *regexp; /* The regex for patterns */
};

struct _xmlSchemaFacetLink {
    struct _xmlSchemaFacetLink *next;/* the next facet link ... */
    xmlSchemaFacet *facet;/* the linked facet */
};

struct _xmlSchemaNotation {
    xmlSchemaTypeType type; /* The kind of type */
    const xmlChar *name;
    xmlSchemaAnnot *annot;
    const xmlChar *identifier;
    const xmlChar *targetNamespace;
};

struct _xmlSchemaType {
    xmlSchemaTypeType type; /* The kind of type */
    struct _xmlSchemaType *next; /* the next type if in a sequence ... */
    const xmlChar *name;
    const xmlChar *id ; /* Deprecated; not used */
    const xmlChar *ref; /* Deprecated; not used */
    const xmlChar *refNs; /* Deprecated; not used */
    xmlSchemaAnnot *annot;
    xmlSchemaType *subtypes;
    xmlSchemaAttribute *attributes; /* Deprecated; not used */
    xmlNode *node;
    int minOccurs; /* Deprecated; not used */
    int maxOccurs; /* Deprecated; not used */

    int flags;
    xmlSchemaContentType contentType;
    const xmlChar *base; /* Base type's local name */
    const xmlChar *baseNs; /* Base type's target namespace */
    xmlSchemaType *baseType; /* The base type component */
    xmlSchemaFacet *facets; /* Local facets */
    struct _xmlSchemaType *redef; /* Deprecated; not used */
    int recurse; /* Obsolete */
    xmlSchemaAttributeLink **attributeUses; /* Deprecated; not used */
    xmlSchemaWildcard *attributeWildcard;
    int builtInType; /* Type of built-in types. */
    xmlSchemaTypeLink *memberTypes; /* member-types if a union type. */
    xmlSchemaFacetLink *facetSet; /* All facets (incl. inherited) */
    const xmlChar *refPrefix; /* Deprecated; not used */
    xmlSchemaType *contentTypeDef; /* Used for the simple content of complex types.
                                        Could we use @subtypes for this? */
    xmlRegexp *contModel; /* Holds the automaton of the content model */
    const xmlChar *targetNamespace;
    void *attrUses;
};

struct _xmlSchemaTypeLink {
    struct _xmlSchemaTypeLink *next;/* the next type link ... */
    xmlSchemaType *type;/* the linked type */
};

struct _xmlSchemaWildcard {
    xmlSchemaTypeType type;        /* The kind of type */
    const xmlChar *id; /* Deprecated; not used */
    xmlSchemaAnnot *annot;
    xmlNode *node;
    int minOccurs; /* Deprecated; not used */
    int maxOccurs; /* Deprecated; not used */
    int processContents;
    int any; /* Indicates if the ns constraint is of ##any */
    xmlSchemaWildcardNs *nsSet; /* The list of allowed namespaces */
    xmlSchemaWildcardNs *negNsSet; /* The negated namespace */
    int flags;
};

struct _xmlSchemaWildcardNs {
    struct _xmlSchemaWildcardNs *next;/* the next constraint link ... */
    const xmlChar *value;/* the value */
};

/* [11.1-G] end: extracted definitions */
#ifdef __cplusplus
}
#endif

#endif /* __XML_SCHEMAS_INTERNALS_H__ */
