/**
 * @file
 *
 * Document tree API for libxml-rs
 */

#ifndef __XML_TREE_H__
#define __XML_TREE_H__

#include <stdio.h>
#include <libxml/xmlversion.h>
#include <libxml/xmlstring.h>
#include <libxml/xmlmemory.h>
#include <libxml/xmlregexp.h>

#ifdef __cplusplus
extern "C" {
#endif




































































































































/* Forward declarations */
struct _xmlParserInputBuffer;
typedef struct _xmlParserInputBuffer xmlParserInputBuffer;
typedef struct _xmlParserInput xmlParserInput;
typedef struct _xmlParserCtxt xmlParserCtxt;
typedef xmlParserInputBuffer *xmlParserInputBufferPtr;

struct _xmlOutputBuffer;
typedef struct _xmlOutputBuffer xmlOutputBuffer;
typedef xmlOutputBuffer *xmlOutputBufferPtr;

typedef struct _xmlBuffer xmlBuffer;
typedef xmlBuffer *xmlBufferPtr;

typedef struct _xmlBuf xmlBuf;
typedef xmlBuf *xmlBufPtr;

typedef struct _xmlNode xmlNode;
typedef xmlNode *xmlNodePtr;

typedef struct _xmlDoc xmlDoc;
typedef xmlDoc *xmlDocPtr;

typedef struct _xmlNs xmlNs;
typedef xmlNs *xmlNsPtr;

typedef struct _xmlAttr xmlAttr;
typedef xmlAttr *xmlAttrPtr;

typedef struct _xmlDtd xmlDtd;
typedef xmlDtd *xmlDtdPtr;

typedef struct _xmlEntity xmlEntity;
typedef xmlEntity *xmlEntityPtr;

typedef struct _xmlNotation xmlNotation;
typedef xmlNotation *xmlNotationPtr;

typedef struct _xmlElementContent xmlElementContent;
typedef xmlElementContent *xmlElementContentPtr;

typedef struct _xmlAttribute xmlAttribute;
typedef xmlAttribute *xmlAttributePtr;

typedef struct _xmlEnumeration xmlEnumeration;
typedef xmlEnumeration *xmlEnumerationPtr;

typedef struct _xmlID xmlID;
typedef xmlID *xmlIDPtr;

typedef struct _xmlParserNodeInfo xmlParserNodeInfo;
typedef xmlParserNodeInfo *xmlParserNodeInfoPtr;

struct _xmlParserNodeInfo {
    const struct _xmlNode *node;
    unsigned long begin_pos;
    unsigned long begin_line;
    unsigned long end_pos;
    unsigned long end_line;
};

typedef struct _xmlParserNodeInfoSeq xmlParserNodeInfoSeq;

struct _xmlParserNodeInfoSeq {
    unsigned long magic;
    xmlParserNodeInfo *buffer;
    int size;
    int number;
};

/* Element types */
typedef enum {
    XML_ELEMENT_NODE = 1,
    XML_ATTRIBUTE_NODE = 2,
    XML_TEXT_NODE = 3,
    XML_CDATA_SECTION_NODE = 4,
    XML_ENTITY_REF_NODE = 5,
    XML_ENTITY_NODE = 6,
    XML_PI_NODE = 7,
    XML_COMMENT_NODE = 8,
    XML_DOCUMENT_NODE = 9,
    XML_DOCUMENT_TYPE_NODE = 10,
    XML_DOCUMENT_FRAG_NODE = 11,
    XML_NOTATION_NODE = 12,
    XML_HTML_DOCUMENT_NODE = 13,
    XML_DTD_NODE = 14,
    XML_ELEMENT_DECL = 15,
    XML_ATTRIBUTE_DECL = 16,
    XML_ENTITY_DECL = 17,
    XML_NAMESPACE_DECL = 18,
    XML_XINCLUDE_START = 19,
    XML_XINCLUDE_END = 20
} xmlElementType;

typedef enum {
    XML_ATTRIBUTE_CDATA = 1,
    XML_ATTRIBUTE_ID = 2,
    XML_ATTRIBUTE_IDREF = 3,
    XML_ATTRIBUTE_IDREFS = 4,
    XML_ATTRIBUTE_ENTITY = 5,
    XML_ATTRIBUTE_ENTITIES = 6,
    XML_ATTRIBUTE_NMTOKEN = 7,
    XML_ATTRIBUTE_NMTOKENS = 8,
    XML_ATTRIBUTE_ENUMERATION = 9,
    XML_ATTRIBUTE_NOTATION = 10
} xmlAttributeType;

typedef enum {
    XML_ATTRIBUTE_NONE = 1,
    XML_ATTRIBUTE_REQUIRED = 2,
    XML_ATTRIBUTE_IMPLIED = 3,
    XML_ATTRIBUTE_FIXED = 4
} xmlAttributeDefault;

typedef enum {
    XML_INTERNAL_GENERAL_ENTITY = 1,
    XML_EXTERNAL_GENERAL_PARSED_ENTITY = 2,
    XML_EXTERNAL_GENERAL_UNPARSED_ENTITY = 3,
    XML_INTERNAL_PARAMETER_ENTITY = 4,
    XML_EXTERNAL_PARAMETER_ENTITY = 5,
    XML_INTERNAL_PREDEFINED_ENTITY = 6
} xmlEntityType;

typedef enum {
    XML_BUFFER_ALLOC_DOUBLEIT,
    XML_BUFFER_ALLOC_EXACT,
    XML_BUFFER_ALLOC_IMMUTABLE,
    XML_BUFFER_ALLOC_IO,
    XML_BUFFER_ALLOC_HYBRID,
    XML_BUFFER_ALLOC_BOUNDED
} xmlBufferAllocationScheme;

/* Document properties */

/* Well-known namespaces */
#define XML_XML_NAMESPACE ((const xmlChar *) "http://www.w3.org/XML/1998/namespace")
#define XML_XMLNS_NAMESPACE ((const xmlChar *) "http://www.w3.org/2000/xmlns/")
#define XML_XMLNS_PREFIX ((const xmlChar *) "xmlns")

/* Limits */
#define XML_MAX_TEXT_LENGTH 1000000000
#define XML_MAX_NAME_LENGTH 50000
#define XML_MAX_DICTIONARY_LIMIT 10000000
#define XML_MAX_LOOKUP_LIMIT 1000000
#define XML_MAX_HUGE_LENGTH 100000000
#define XML_MAX_NAMELEN 50000
#define XML_MAX_ATTRIBUTE_LENGTH 50000

/* Buffer structure */
struct _xmlBuffer {
    xmlChar *content;
    unsigned int use;
    unsigned int size;
    xmlBufferAllocationScheme alloc;
    xmlChar *contentIO;
};

/* Node structure */
struct _xmlNode {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    xmlNsPtr ns;
    xmlChar *content;
    xmlAttrPtr properties;
    xmlNsPtr nsDef;
    void *psvi;
    unsigned short line;
    unsigned short extra;
};

/* Attribute structure */
struct _xmlAttr {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlAttrPtr next;
    xmlAttrPtr prev;
    xmlDocPtr doc;
    xmlNsPtr ns;
    int atype;
    void *psvi;
    xmlIDPtr id;
};

/* Namespace structure */
struct _xmlNs {
    xmlNsPtr next;
    int type;
    xmlChar *href;
    xmlChar *prefix;
    void *_private;
    void *context;
};

/* Document structure */
struct _xmlDoc {
    void *_private;
    int type;
    char *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    int compression;
    int standalone;
    xmlDtdPtr intSubset;
    xmlDtdPtr extSubset;
    xmlNsPtr oldNs;
    const xmlChar *version;
    const xmlChar *encoding;
    void *ids;
    void *refs;
    const xmlChar *URL;
    int charset;
    void *dict;
    void *psvi;
    int parseFlags;
    int properties;
};

/* DTD structure */
struct _xmlDtd {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    void *notations;
    void *elements;
    void *attributes;
    void *entities;
    const xmlChar *ExternalID;
    const xmlChar *SystemID;
    void *pentities;
};

/* Entity structure */
struct _xmlEntity {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    xmlChar *orig;
    xmlChar *content;
    int length;
    int etype;
    const xmlChar *ExternalID;
    const xmlChar *SystemID;
    xmlEntityPtr nexte;
    const xmlChar *URI;
    int owner;
    int flags;
    unsigned long expandedSize;
};

/* Notation structure */
struct _xmlNotation {
    const xmlChar *name;
    const xmlChar *PublicID;
    const xmlChar *SystemID;
};

/* Element content structure */
struct _xmlElementContent {
    int type;
    int ocur;
    const xmlChar *name;
    xmlElementContentPtr c1;
    xmlElementContentPtr c2;
    xmlElementContentPtr parent;
    const xmlChar *prefix;
};

/* Enumeration structure */
struct _xmlEnumeration {
    struct _xmlEnumeration *next;
    const xmlChar *name;
};

/* Attribute declaration structure */
struct _xmlAttribute {
    void *_private;
    int type;
    const xmlChar *name;
    xmlNodePtr children;
    xmlNodePtr last;
    xmlNodePtr parent;
    xmlNodePtr next;
    xmlNodePtr prev;
    xmlDocPtr doc;
    xmlAttributePtr nexth;
    int atype;
    int def;
    const xmlChar *defaultValue;
    xmlEnumerationPtr tree;
    const xmlChar *prefix;
    const xmlChar *elem;
};

/* Element declaration types */
typedef enum {
    XML_ELEMENT_TYPE_UNDEFINED = 0,
    XML_ELEMENT_TYPE_EMPTY = 1,
    XML_ELEMENT_TYPE_ANY = 2,
    XML_ELEMENT_TYPE_MIXED = 3,
    XML_ELEMENT_TYPE_ELEMENT = 4
} xmlElementTypeVal;

/* Namespace type */
#define XML_LOCAL_NAMESPACE 0
typedef int xmlNsType;

/* Tree functions */
XMLPUBFUN xmlDocPtr xmlNewDoc(const xmlChar *version);
XMLPUBFUN void xmlFreeDoc(xmlDocPtr doc);
XMLPUBFUN xmlDocPtr xmlCopyDoc(const xmlDocPtr doc, int recursive);
XMLPUBFUN xmlNodePtr xmlNewNode(xmlNsPtr ns, const xmlChar *name);
XMLPUBFUN void xmlFreeNode(xmlNodePtr node);
XMLPUBFUN void xmlFreeNodeList(xmlNodePtr node);
XMLPUBFUN xmlNodePtr xmlCopyNode(const xmlNodePtr node, int extended);
XMLPUBFUN void xmlUnlinkNode(xmlNodePtr node);
XMLPUBFUN xmlNodePtr xmlAddChild(xmlNodePtr parent, xmlNodePtr cur);
XMLPUBFUN xmlNodePtr xmlAddSibling(xmlNodePtr cur, xmlNodePtr sibling);
XMLPUBFUN xmlNodePtr xmlNewChild(xmlNodePtr parent, xmlNsPtr ns,
                                  const xmlChar *name, const xmlChar *content);
XMLPUBFUN xmlNodePtr xmlNewText(const xmlChar *content);
XMLPUBFUN xmlNodePtr xmlNewComment(const xmlChar *content);
XMLPUBFUN xmlNodePtr xmlNewPI(const xmlChar *name, const xmlChar *content);
XMLPUBFUN xmlNodePtr xmlNewCDataBlock(xmlDocPtr doc, const xmlChar *content, int len);
XMLPUBFUN xmlNodePtr xmlDocSetRootElement(xmlDocPtr doc, xmlNodePtr root);
XMLPUBFUN xmlNodePtr xmlDocGetRootElement(const xmlDoc *doc);
XMLPUBFUN long xmlGetLineNo(const xmlNode *node);
XMLPUBFUN xmlNodePtr xmlFirstElementChild(xmlNodePtr parent);
XMLPUBFUN xmlNodePtr xmlLastElementChild(xmlNodePtr parent);
XMLPUBFUN xmlNodePtr xmlNextElementSibling(xmlNodePtr node);
XMLPUBFUN xmlNodePtr xmlPreviousElementSibling(xmlNodePtr node);
XMLPUBFUN int xmlNodeIsText(const xmlNode *node);
XMLPUBFUN int xmlIsBlankNode(const xmlNode *node);
XMLPUBFUN xmlNsPtr xmlNewNs(xmlNodePtr node, const xmlChar *href, const xmlChar *prefix);
XMLPUBFUN void xmlSetNs(xmlNodePtr node, xmlNsPtr ns);
XMLPUBFUN xmlNsPtr *xmlGetNsList(xmlDocPtr doc, const xmlNode *node);
XMLPUBFUN xmlNsPtr xmlSearchNs(xmlDocPtr doc, xmlNodePtr node, const xmlChar *nameSpace);
XMLPUBFUN xmlNsPtr xmlSearchNsByHref(xmlDocPtr doc, xmlNodePtr node, const xmlChar *href);
XMLPUBFUN xmlAttrPtr xmlSetProp(xmlNodePtr node, const xmlChar *name, const xmlChar *value);
XMLPUBFUN xmlChar *xmlGetProp(const xmlNode *node, const xmlChar *name);
XMLPUBFUN xmlChar *xmlNodeGetContent(const xmlNode *cur);
XMLPUBFUN xmlChar *xmlNodeGetBase(const xmlDoc *doc, const xmlNode *cur);
XMLPUBFUN void xmlNodeSetContent(xmlNodePtr cur, const xmlChar *content);
XMLPUBFUN void xmlNodeSetName(xmlNodePtr cur, const xmlChar *name);
XMLPUBFUN void xmlSetTreeDoc(xmlNodePtr tree, xmlDocPtr doc);
XMLPUBFUN void xmlSetListDoc(xmlNodePtr list, xmlDocPtr doc);
XMLPUBFUN xmlAttrPtr xmlHasProp(const xmlNode *node, const xmlChar *name);
XMLPUBFUN xmlNodePtr xmlDocCopyNode(const xmlNodePtr node, xmlDocPtr doc, int extended);
XMLPUBFUN xmlNodePtr xmlCopyNodeList(const xmlNodePtr node);
XMLPUBFUN xmlAttrPtr xmlCopyProp(xmlNodePtr target, const xmlAttrPtr cur);
XMLPUBFUN xmlAttrPtr xmlCopyPropList(xmlNodePtr target, const xmlAttrPtr cur);
XMLPUBFUN xmlChar *xmlGetNsProp(const xmlNode *node, const xmlChar *name, const xmlChar *nameSpace);
XMLPUBFUN xmlAttrPtr xmlSetNsProp(xmlNodePtr node, xmlNsPtr ns,
                                   const xmlChar *name, const xmlChar *value);
XMLPUBFUN int xmlRemoveProp(xmlAttrPtr attr);
XMLPUBFUN xmlDtdPtr xmlGetIntSubset(const xmlDoc *doc);
XMLPUBFUN xmlDtdPtr xmlNewDtd(xmlDocPtr doc, const xmlChar *name,
                               const xmlChar *ExternalID, const xmlChar *SystemID);
XMLPUBFUN xmlEntityPtr xmlNewEntity(xmlDocPtr doc, const xmlChar *name, int type,
                                     const xmlChar *ExternalID, const xmlChar *SystemID,
                                     const xmlChar *content);
XMLPUBFUN xmlEntityPtr xmlGetDocEntity(const xmlDoc *doc, const xmlChar *name);
XMLPUBFUN xmlEntityPtr xmlGetParameterEntity(const xmlDoc *doc, const xmlChar *name);
XMLPUBFUN xmlBufferPtr xmlBufferCreate(void);
XMLPUBFUN xmlBufferPtr xmlBufferCreateSize(size_t size);
XMLPUBFUN xmlBufferPtr xmlBufferCreateStatic(void *mem, size_t size);
XMLPUBFUN void xmlBufferFree(xmlBufferPtr buf);
XMLPUBFUN void xmlBufferEmpty(xmlBufferPtr buf);
XMLPUBFUN xmlChar *xmlBufferContent(const xmlBuffer *buf);
XMLPUBFUN int xmlBufferLength(const xmlBuffer *buf);
XMLPUBFUN int xmlBufferAdd(xmlBufferPtr buf, const xmlChar *str, int len);
XMLPUBFUN int xmlBufferAddHead(xmlBufferPtr buf, const xmlChar *str, int len);
XMLPUBFUN void xmlBufferSetAllocationScheme(xmlBufferPtr buf, int scheme);
XMLPUBFUN int xmlBufferShrink(xmlBufferPtr buf, int len);
XMLPUBFUN int xmlBufferGrow(xmlBufferPtr buf, int len);
XMLPUBFUN int xmlBufferReserve(xmlBufferPtr buf, int len);
XMLPUBFUN xmlChar *xmlBufferDetach(xmlBufferPtr buf);
XMLPUBFUN int xmlIsBlankNode(const xmlNode *node);
















































































































































































































/* [11.1-G] begin: extracted verbatim from upstream oracle header */
typedef struct _xmlDOMWrapCtxt xmlDOMWrapCtxt;
typedef xmlDOMWrapCtxt *xmlDOMWrapCtxtPtr;

/**
 * A function called to acquire namespaces (xmlNs) from the wrapper.
 *
 * @param ctxt  a DOM wrapper context
 * @param node  the context node (element or attribute)
 * @param nsName  the requested namespace name
 * @param nsPrefix  the requested namespace prefix
 * @returns an xmlNs or NULL in case of an error.
 */
typedef xmlNs *(*xmlDOMWrapAcquireNsFunction) (xmlDOMWrapCtxt *ctxt,
						 xmlNode *node,
						 const xmlChar *nsName,
						 const xmlChar *nsPrefix);

/**
 * Context for DOM wrapper-operations.
 */

typedef struct _xmlElement xmlElement;
typedef xmlElement *xmlElementPtr;
/**
 * An XML Element declaration from a DTD.
 *
 * Should be treated as opaque. Accessing members directly
 * is deprecated.
 */

typedef struct _xmlRef xmlRef;
typedef xmlRef *xmlRefPtr;
/*
 * An XML IDREF instance.
 */

typedef enum{
    XML_DOC_WELLFORMED		= 1<<0,
    XML_DOC_NSVALID		= 1<<1,
    XML_DOC_OLD10		= 1<<2,
    XML_DOC_DTDVALID		= 1<<3,
    XML_DOC_XINCLUDE		= 1<<4,
    XML_DOC_USERBUILT		= 1<<5,
    XML_DOC_INTERNAL		= 1<<6,
    XML_DOC_HTML		= 1<<7
} xmlDocProperties;

typedef enum{
    XML_ELEMENT_CONTENT_ONCE = 1,
    XML_ELEMENT_CONTENT_OPT,
    XML_ELEMENT_CONTENT_MULT,
    XML_ELEMENT_CONTENT_PLUS
} xmlElementContentOccur;

typedef enum{
    XML_ELEMENT_CONTENT_PCDATA = 1,
    XML_ELEMENT_CONTENT_ELEMENT,
    XML_ELEMENT_CONTENT_SEQ,
    XML_ELEMENT_CONTENT_OR
} xmlElementContentType;

struct _xmlDOMWrapCtxt {
    void * _private;
    /*
    * The type of this context, just in case we need specialized
    * contexts in the future.
    */
    int type;
    /*
    * Internal namespace map used for various operations.
    */
    void * namespaceMap;
    /*
    * Use this one to acquire an xmlNs intended for node->ns.
    * (Note that this is not intended for elem->nsDef).
    */
    xmlDOMWrapAcquireNsFunction getNsForNodeFunc;
};

struct _xmlElement {
    /** application data */
    void           *_private;
    /** XML_ELEMENT_DECL */
    xmlElementType          type;
    /** element name */
    const xmlChar          *name;
    /** NULL */
    struct _xmlNode    *children;
    /** NULL */
    struct _xmlNode        *last;
    /** -> DTD */
    struct _xmlDtd       *parent;
    /** next sibling */
    struct _xmlNode        *next;
    /** previous sibling */
    struct _xmlNode        *prev;
    /** containing document */
    struct _xmlDoc          *doc;

    /** element type */
    xmlElementTypeVal      etype XML_DEPRECATED_MEMBER;
    /** allowed element content */
    xmlElementContent *content XML_DEPRECATED_MEMBER;
    /** list of declared attributes */
    xmlAttribute     *attributes XML_DEPRECATED_MEMBER;
    /** namespace prefix if any */
    const xmlChar        *prefix XML_DEPRECATED_MEMBER;
#ifdef LIBXML_REGEXP_ENABLED
    /** validating regexp */
    xmlRegexp         *contModel XML_DEPRECATED_MEMBER;
#else
    void	      *contModel XML_DEPRECATED_MEMBER;
#endif
};

struct _xmlID {
    /* next ID */
    struct _xmlID    *next XML_DEPRECATED_MEMBER;
    /* The ID name */
    xmlChar *value XML_DEPRECATED_MEMBER;
    /* The attribute holding it */
    xmlAttr          *attr XML_DEPRECATED_MEMBER;
    /* The attribute if attr is not available */
    const xmlChar    *name XML_DEPRECATED_MEMBER;
    /* The line number if attr is not available */
    int               lineno XML_DEPRECATED_MEMBER;
    /* The document holding the ID */
    struct _xmlDoc   *doc XML_DEPRECATED_MEMBER;
};

struct _xmlRef {
    /* next Ref */
    struct _xmlRef    *next XML_DEPRECATED_MEMBER;
    /* The Ref name */
    const xmlChar     *value XML_DEPRECATED_MEMBER;
    /* The attribute holding it */
    xmlAttr          *attr XML_DEPRECATED_MEMBER;
    /* The attribute if attr is not available */
    const xmlChar    *name XML_DEPRECATED_MEMBER;
    /* The line number if attr is not available */
    int               lineno XML_DEPRECATED_MEMBER;
};

/* [11.1-G] end: extracted definitions */

/* [11.1-L] begin: node-registration callback declarations extracted verbatim
 * from the oracle libxml2 2.15.3 tree.h (exported by the candidate DSO). */
typedef void (*xmlRegisterNodeFunc) (xmlNode *node);
typedef void (*xmlDeregisterNodeFunc) (xmlNode *node);
XMLPUBFUN xmlRegisterNodeFunc
	    xmlRegisterNodeDefault	(xmlRegisterNodeFunc func);
XMLPUBFUN xmlDeregisterNodeFunc
	    xmlDeregisterNodeDefault	(xmlDeregisterNodeFunc func);
/* [11.1-L] end: extracted declarations */
#ifdef __cplusplus
}
#endif

#endif /* __XML_TREE_H__ */
