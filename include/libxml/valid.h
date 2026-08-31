/**
 * @file
 *
 * DTD validation API for libxml-rs
 *
 * # UPSTREAM-PARITY
 * `struct _xmlValidCtxt` layout matches upstream `valid.h` (libxml2 2.15.x)
 * and is embedded by value in `xmlParserCtxt`.
 */

#ifndef __XML_VALID_H__
#define __XML_VALID_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>
#include <libxml/list.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlValidCtxt xmlValidCtxt;
typedef xmlValidCtxt *xmlValidCtxtPtr;

typedef struct _xmlHashTable xmlIDTable;
typedef xmlIDTable *xmlIDTablePtr;
typedef struct _xmlHashTable xmlRefTable;
typedef xmlRefTable *xmlRefTablePtr;

/**
 * xmlValidityErrorFunc:
 *
 * Signature of the error callback for validation (upstream valid.h).
 */
typedef void (*xmlValidityErrorFunc) (void *ctx, const char *msg, ...);
typedef void (*xmlValidityWarningFunc) (void *ctx, const char *msg, ...);

/**
 * xmlValidCtxt:
 *
 * Validation context; embedded by value in xmlParserCtxt (upstream layout).
 */
struct _xmlValidCtxt {
    void *userData;
    xmlValidityErrorFunc error;
    xmlValidityWarningFunc warning;
    xmlNodePtr node;
    int nodeNr;
    int nodeMax;
    xmlNodePtr *nodeTab;
    unsigned int flags;
    xmlDocPtr doc;
    int valid;
    void *vstate;
    int vstateNr;
    int vstateMax;
    void *vstateTab;
    void *am;
    void *state;
};

/*
 * Validation functions (candidate exports)
 */
XMLPUBFUN int
                xmlValidateDocument       (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc);
XMLPUBFUN int
                xmlValidateDocumentFinal  (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc);
XMLPUBFUN int
                xmlValidateElement        (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlNodePtr elem);
XMLPUBFUN int
                xmlValidateAttributeDecl  (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlAttributePtr attr);
XMLPUBFUN int
                xmlValidateAttributeValue (xmlAttributeType type,
                                           const xmlChar *value);
XMLPUBFUN int
                xmlValidateNotationUse    (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           const xmlChar *notationName);
XMLPUBFUN int
                xmlValidateID             (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlNodePtr elem,
                                           const xmlChar *value);
XMLPUBFUN int
                xmlValidateIDRef          (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           const xmlChar *value);
XMLPUBFUN int
                xmlValidateIDRefs         (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           const xmlChar *value);
XMLPUBFUN int
                xmlValidateNameValue      (const xmlChar *value);
XMLPUBFUN int
                xmlValidateNamesValue     (const xmlChar *value);
XMLPUBFUN int
                xmlValidateNmtokenValue   (const xmlChar *value);
XMLPUBFUN int
                xmlValidateNmtokensValue  (const xmlChar *value);
XMLPUBFUN int
                xmlValidateNotationDecl   (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlNotationPtr nota);
XMLPUBFUN int
                xmlValidateElementDecl    (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlElementPtr elem);
XMLPUBFUN int
                xmlValidateOneAttribute   (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlNodePtr elem,
                                           xmlAttrPtr attr,
                                           const xmlChar *value);
XMLPUBFUN int
                xmlValidateOneElement     (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlNodePtr elem);
XMLPUBFUN int
                xmlValidateOneNamespace   (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlNodePtr elem,
                                           const xmlChar *prefix,
                                           xmlNsPtr ns,
                                           const xmlChar *value);
XMLPUBFUN int
                xmlValidateRoot           (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc);
XMLPUBFUN int
                xmlValidBuildContentModel (xmlValidCtxtPtr ctxt,
                                           xmlElementPtr elem);
XMLPUBFUN int
                xmlValidatePushElement    (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlNodePtr elem,
                                           const xmlChar *qname);
XMLPUBFUN int
                xmlValidatePushCData      (xmlValidCtxtPtr ctxt,
                                           const xmlChar *data,
                                           int len);
XMLPUBFUN int
                xmlValidatePopElement     (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlNodePtr elem,
                                           const xmlChar *qname);

/* ID / IDREF tables */
XMLPUBFUN xmlID *
                xmlAddID                  (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           const xmlChar *value,
                                           xmlAttrPtr attr);
XMLPUBFUN int
                xmlAddIDSafe              (xmlAttrPtr attr,
                                           const xmlChar *value);
XMLPUBFUN int
                xmlRemoveID               (xmlDocPtr doc,
                                           xmlAttrPtr attr);
XMLPUBFUN xmlAttr *
                xmlGetID                  (xmlDocPtr doc,
                                           const xmlChar *ID);
XMLPUBFUN void
                xmlFreeIDTable            (xmlIDTablePtr table);
XMLPUBFUN xmlRef *
                xmlAddRef                 (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           const xmlChar *value,
                                           xmlAttrPtr attr);
XMLPUBFUN int
                xmlRemoveRef              (xmlDocPtr doc,
                                           xmlAttrPtr attr);
XMLPUBFUN void
                xmlFreeRefTable           (xmlRefTablePtr table);
XMLPUBFUN xmlList *
                xmlGetRefs                (xmlDocPtr doc,
                                           const xmlChar *ID);
XMLPUBFUN int
                xmlIsID                   (xmlDocPtr doc,
                                           xmlNodePtr elem,
                                           xmlAttrPtr attr);
XMLPUBFUN int
                xmlIsRef                  (xmlDocPtr doc,
                                           xmlNodePtr elem,
                                           xmlAttrPtr attr);
XMLPUBFUN xmlElement *
                xmlGetDtdElementDesc      (xmlDtdPtr dtd,
                                           const xmlChar *name);
XMLPUBFUN xmlAttribute *
                xmlGetDtdAttrDesc         (xmlDtdPtr dtd,
                                           const xmlChar *elem,
                                           const xmlChar *name);
XMLPUBFUN xmlElement *
                xmlGetDtdQElementDesc     (xmlDtdPtr dtd,
                                           const xmlChar *name,
                                           const xmlChar *prefix);
XMLPUBFUN xmlAttribute *
                xmlGetDtdQAttrDesc        (xmlDtdPtr dtd,
                                           const xmlChar *elem,
                                           const xmlChar *name,
                                           const xmlChar *prefix);
XMLPUBFUN xmlNotation *
                xmlGetDtdNotationDesc     (xmlDtdPtr dtd,
                                           const xmlChar *name);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN xmlAttribute * xmlAddAttributeDecl (xmlValidCtxt *ctxt, xmlDtd *dtd, const xmlChar *elem, const xmlChar *name, const xmlChar *ns, xmlAttributeType type, xmlAttributeDefault def, const xmlChar *defaultValue, xmlEnumeration *tree);
XMLPUBFUN xmlElement * xmlAddElementDecl (xmlValidCtxt *ctxt, xmlDtd *dtd, const xmlChar *name, xmlElementTypeVal type, xmlElementContent *content);
XMLPUBFUN xmlNotation * xmlAddNotationDecl (xmlValidCtxt *ctxt, xmlDtd *dtd, const xmlChar *name, const xmlChar *publicId, const xmlChar *systemId);
XMLPUBFUN xmlAttributeTable * xmlCopyAttributeTable (xmlAttributeTable *table);
XMLPUBFUN xmlElementContent * xmlCopyDocElementContent(xmlDoc *doc, xmlElementContent *content);
XMLPUBFUN xmlElementContent * xmlCopyElementContent (xmlElementContent *content);
XMLPUBFUN xmlElementTable * xmlCopyElementTable (xmlElementTable *table);
XMLPUBFUN xmlEnumeration * xmlCopyEnumeration (xmlEnumeration *cur);
XMLPUBFUN xmlNotationTable * xmlCopyNotationTable (xmlNotationTable *table);
XMLPUBFUN xmlEnumeration * xmlCreateEnumeration (const xmlChar *name);
XMLPUBFUN void xmlDumpAttributeDecl (xmlBuffer *buf, xmlAttribute *attr);
XMLPUBFUN void xmlDumpAttributeTable (xmlBuffer *buf, xmlAttributeTable *table);
XMLPUBFUN void xmlDumpElementDecl (xmlBuffer *buf, xmlElement *elem);
XMLPUBFUN void xmlDumpElementTable (xmlBuffer *buf, xmlElementTable *table);
XMLPUBFUN void xmlDumpNotationDecl (xmlBuffer *buf, xmlNotation *nota);
XMLPUBFUN void xmlDumpNotationTable (xmlBuffer *buf, xmlNotationTable *table);
XMLPUBFUN void xmlFreeAttributeTable (xmlAttributeTable *table);
XMLPUBFUN void xmlFreeDocElementContent(xmlDoc *doc, xmlElementContent *cur);
XMLPUBFUN void xmlFreeElementContent (xmlElementContent *cur);
XMLPUBFUN void xmlFreeElementTable (xmlElementTable *table);
XMLPUBFUN void xmlFreeEnumeration (xmlEnumeration *cur);
XMLPUBFUN void xmlFreeNotationTable (xmlNotationTable *table);
XMLPUBFUN void xmlFreeValidCtxt(xmlValidCtxt *);
XMLPUBFUN int xmlIsMixedElement (xmlDoc *doc, const xmlChar *name);
XMLPUBFUN xmlElementContent * xmlNewDocElementContent (xmlDoc *doc, const xmlChar *name, xmlElementContentType type);
XMLPUBFUN xmlElementContent * xmlNewElementContent (const xmlChar *name, xmlElementContentType type);
XMLPUBFUN xmlValidCtxt * xmlNewValidCtxt(void);
XMLPUBFUN void xmlSnprintfElementContent(char *buf, int size, xmlElementContent *content, int englob);
XMLPUBFUN void xmlSprintfElementContent(char *buf, xmlElementContent *content, int englob);
XMLPUBFUN xmlChar * xmlValidCtxtNormalizeAttributeValue(xmlValidCtxt *ctxt, xmlDoc *doc, xmlNode *elem, const xmlChar *name, const xmlChar *value);
XMLPUBFUN int xmlValidGetPotentialChildren(xmlElementContent *ctree, const xmlChar **names, int *len, int max);
XMLPUBFUN int xmlValidGetValidElements(xmlNode *prev, xmlNode *next, const xmlChar **names, int max);
XMLPUBFUN xmlChar * xmlValidNormalizeAttributeValue(xmlDoc *doc, xmlNode *elem, const xmlChar *name, const xmlChar *value);
XMLPUBFUN int xmlValidateDtd (xmlValidCtxt *ctxt, xmlDoc *doc, xmlDtd *dtd);
XMLPUBFUN int xmlValidateDtdFinal (xmlValidCtxt *ctxt, xmlDoc *doc);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_VALID_H__ */
