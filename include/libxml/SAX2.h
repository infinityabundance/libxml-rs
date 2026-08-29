/**
 * @file
 *
 * SAX2 API for libxml-rs
 */

#ifndef __XML_SAX2_H__
#define __XML_SAX2_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

/* Forward declarations for types defined in parser.h */
struct _xmlParserInput;
typedef struct _xmlParserInput xmlParserInput;
typedef xmlParserInput *xmlParserInputPtr;


#ifdef __cplusplus
extern "C" {
#endif

#define XML_SAX2_MAGIC 0xDEEDBEAF

/* SAX locator - must be defined before SAX2 callback types */
typedef struct _xmlSAXLocator xmlSAXLocator;
typedef xmlSAXLocator *xmlSAXLocatorPtr;
struct _xmlSAXLocator {
    xmlChar *(*getPublicId)(void *ctx);
    xmlChar *(*getSystemId)(void *ctx);
    int (*getLineNumber)(void *ctx);
    int (*getColumnNumber)(void *ctx);
};

/* SAX2 callback types */
typedef void (*startDocumentSAXFunc)(void *ctx);
typedef void (*endDocumentSAXFunc)(void *ctx);
typedef void (*startElementSAXFunc)(void *ctx, const xmlChar *name,
                                     const xmlChar **atts);
typedef void (*endElementSAXFunc)(void *ctx, const xmlChar *name);
typedef void (*charactersSAXFunc)(void *ctx, const xmlChar *ch, int len);
typedef void (*processingInstructionSAXFunc)(void *ctx,
                                              const xmlChar *target,
                                              const xmlChar *data);
typedef void (*commentSAXFunc)(void *ctx, const xmlChar *value);
typedef void (*warningSAXFunc)(void *ctx, const char *msg, ...);
typedef void (*errorSAXFunc)(void *ctx, const char *msg, ...);
typedef void (*fatalErrorSAXFunc)(void *ctx, const char *msg, ...);
typedef void (*cdataBlockSAXFunc)(void *ctx, const xmlChar *value, int len);
typedef void (*referenceSAXFunc)(void *ctx, const xmlChar *name);
typedef void (*ignorableWhitespaceSAXFunc)(void *ctx, const xmlChar *ch, int len);
typedef void (*setDocumentLocatorSAXFunc)(void *ctx, xmlSAXLocatorPtr loc);
typedef xmlParserInputPtr (*resolveEntitySAXFunc)(void *ctx,
                                                    const xmlChar *publicId,
                                                    const xmlChar *systemId);
typedef xmlEntityPtr (*getEntitySAXFunc)(void *ctx, const xmlChar *name);
typedef xmlEntityPtr (*getParameterEntitySAXFunc)(void *ctx, const xmlChar *name);
typedef void (*entityDeclSAXFunc)(void *ctx, const xmlChar *name, int type,
                                   const xmlChar *publicId, const xmlChar *systemId,
                                   xmlChar *content);
typedef void (*notationDeclSAXFunc)(void *ctx, const xmlChar *name,
                                     const xmlChar *publicId,
                                     const xmlChar *systemId);
typedef void (*attributeDeclSAXFunc)(void *ctx, const xmlChar *elem,
                                      const xmlChar *fullname, int type, int def,
                                      const xmlChar *defaultValue,
                                      xmlEnumerationPtr tree);
typedef void (*elementDeclSAXFunc)(void *ctx, const xmlChar *name, int type,
                                    xmlElementContentPtr content);
typedef void (*unparsedEntityDeclSAXFunc)(void *ctx, const xmlChar *name,
                                           const xmlChar *publicId,
                                           const xmlChar *systemId,
                                           const xmlChar *notationName);
typedef void (*internalSubsetSAXFunc)(void *ctx, const xmlChar *name,
                                       const xmlChar *ExternalID,
                                       const xmlChar *SystemID);
typedef int (*isStandaloneSAXFunc)(void *ctx);
typedef int (*hasInternalSubsetSAXFunc)(void *ctx);
typedef int (*hasExternalSubsetSAXFunc)(void *ctx);
typedef void (*externalSubsetSAXFunc)(void *ctx, const xmlChar *name,
                                       const xmlChar *ExternalID,
                                       const xmlChar *SystemID);

/* SAX2 element handlers */
typedef void (*startElementNsSAX2Func)(void *ctx,
                                        const xmlChar *localname,
                                        const xmlChar *prefix,
                                        const xmlChar *URI,
                                        int nb_namespaces,
                                        const xmlChar **namespaces,
                                        int nb_attributes,
                                        int nb_defaulted,
                                        const xmlChar **attributes);
typedef void (*endElementNsSAX2Func)(void *ctx,
                                      const xmlChar *localname,
                                      const xmlChar *prefix,
                                      const xmlChar *URI);



/* SAX handler structure */
typedef struct _xmlSAXHandler xmlSAXHandler;
typedef xmlSAXHandler *xmlSAXHandlerPtr;
struct _xmlSAXHandler {
    internalSubsetSAXFunc internalSubset;
    isStandaloneSAXFunc isStandalone;
    hasInternalSubsetSAXFunc hasInternalSubset;
    hasExternalSubsetSAXFunc hasExternalSubset;
    resolveEntitySAXFunc resolveEntity;
    getEntitySAXFunc getEntity;
    entityDeclSAXFunc entityDecl;
    notationDeclSAXFunc notationDecl;
    attributeDeclSAXFunc attributeDecl;
    elementDeclSAXFunc elementDecl;
    unparsedEntityDeclSAXFunc unparsedEntityDecl;
    setDocumentLocatorSAXFunc setDocumentLocator;
    startDocumentSAXFunc startDocument;
    endDocumentSAXFunc endDocument;
    startElementSAXFunc startElement;
    endElementSAXFunc endElement;
    referenceSAXFunc reference;
    charactersSAXFunc characters;
    ignorableWhitespaceSAXFunc ignorableWhitespace;
    processingInstructionSAXFunc processingInstruction;
    commentSAXFunc comment;
    warningSAXFunc warning;
    errorSAXFunc error;
    fatalErrorSAXFunc fatalError;
    getParameterEntitySAXFunc getParameterEntity;
    cdataBlockSAXFunc cdataBlock;
    externalSubsetSAXFunc externalSubset;
    unsigned int initialized;
    void *_private;
    startElementNsSAX2Func startElementNs;
    endElementNsSAX2Func endElementNs;
    xmlStructuredErrorFunc serror;
};

XMLPUBFUN void xmlSAX2InitDefaultSAXHandler(xmlSAXHandlerPtr handler, int warning);
XMLPUBFUN void xmlSAX2InitHtmlDefaultSAXHandler(xmlSAXHandlerPtr handler);

#ifdef __cplusplus
}
#endif

#endif /* __XML_SAX2_H__ */
