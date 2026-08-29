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

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlValidCtxt xmlValidCtxt;
typedef xmlValidCtxt *xmlValidCtxtPtr;

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
                xmlValidateNmtoken        (const xmlChar *value);
XMLPUBFUN int
                xmlValidateNmtokens       (const xmlChar *value);
XMLPUBFUN int
                xmlValidateName           (const xmlChar *value);
XMLPUBFUN int
                xmlValidateNameValue      (const xmlChar *value);
XMLPUBFUN int
                xmlValidateNamesValue     (const xmlChar *value);
XMLPUBFUN int
                xmlValidateNotationDecl   (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc,
                                           xmlNodePtr cur);
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
                xmlValidateRoot           (xmlValidCtxtPtr ctxt,
                                           xmlDocPtr doc);

#ifdef __cplusplus
}
#endif

#endif /* __XML_VALID_H__ */
