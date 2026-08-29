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
                                           xmlNodePtr elem,
                                           const xmlChar *fullname);
XMLPUBFUN int
                xmlValidateAttributeValue (int type,
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

#ifdef __cplusplus
}
#endif

#endif /* __XML_VALID_H__ */
