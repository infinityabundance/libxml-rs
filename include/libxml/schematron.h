/**
 * @file
 *
 * Schematron API for libxml-rs
 *
 * # UPSTREAM-PARITY
 * Types follow upstream `schematron.h` (libxml2 2.15.x).
 */

#ifndef __XML_SCHEMATRON_H__
#define __XML_SCHEMATRON_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxml/xmlerror.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Validation callback types (upstream schematron.h) */
typedef void (*xmlSchematronValidityErrorFunc) (void *ctx, const char *msg, ...);
typedef void (*xmlSchematronValidityWarningFunc) (void *ctx, const char *msg, ...);

typedef struct _xmlSchematron xmlSchematron;
typedef xmlSchematron *xmlSchematronPtr;
typedef struct _xmlSchematronParserCtxt xmlSchematronParserCtxt;
typedef xmlSchematronParserCtxt *xmlSchematronParserCtxtPtr;
typedef struct _xmlSchematronValidCtxt xmlSchematronValidCtxt;
typedef xmlSchematronValidCtxt *xmlSchematronValidCtxtPtr;

typedef enum {
    XML_SCHEMATRON_OUT_QUIET = 1 << 0,
    XML_SCHEMATRON_OUT_TEXT = 1 << 1,
    XML_SCHEMATRON_OUT_XML = 1 << 2,
    XML_SCHEMATRON_OUT_ERROR = 1 << 3,
    XML_SCHEMATRON_OUT_ANNOTATE = 1 << 4,
    XML_SCHEMATRON_OUT_STREAM = 1 << 5
} xmlSchematronValidOptions;

XMLPUBFUN xmlSchematronParserCtxtPtr xmlSchematronNewParserCtxt(const char *URL);
XMLPUBFUN xmlSchematronParserCtxtPtr xmlSchematronNewMemParserCtxt(const char *buffer,
                                                                   int size);
XMLPUBFUN xmlSchematronPtr xmlSchematronParse(xmlSchematronParserCtxtPtr ctxt);
XMLPUBFUN void xmlSchematronFreeParserCtxt(xmlSchematronParserCtxtPtr ctxt);
XMLPUBFUN void xmlSchematronFree(xmlSchematronPtr schema);
XMLPUBFUN xmlSchematronValidCtxtPtr xmlSchematronNewValidCtxt(xmlSchematronPtr schema,
                                                              int options);
XMLPUBFUN void xmlSchematronFreeValidCtxt(xmlSchematronValidCtxtPtr ctxt);
XMLPUBFUN int xmlSchematronValidateDoc(xmlSchematronValidCtxtPtr ctxt,
                                       xmlDocPtr instance);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN int xmlSchematronGetParserErrors(xmlSchematronParserCtxt *ctxt, xmlSchematronValidityErrorFunc * err, xmlSchematronValidityWarningFunc * warn, void **ctx);
XMLPUBFUN int xmlSchematronGetValidErrors (xmlSchematronValidCtxt *ctxt, xmlSchematronValidityErrorFunc *err, xmlSchematronValidityWarningFunc *warn, void **ctx);
XMLPUBFUN int xmlSchematronIsValid (xmlSchematronValidCtxt *ctxt);
XMLPUBFUN xmlSchematronParserCtxt * xmlSchematronNewDocParserCtxt(xmlDoc *doc);
XMLPUBFUN void xmlSchematronSetParserErrors(xmlSchematronParserCtxt *ctxt, xmlSchematronValidityErrorFunc err, xmlSchematronValidityWarningFunc warn, void *ctx);
XMLPUBFUN void xmlSchematronSetValidErrors (xmlSchematronValidCtxt *ctxt, xmlSchematronValidityErrorFunc err, xmlSchematronValidityWarningFunc warn, void *ctx);
XMLPUBFUN int xmlSchematronSetValidOptions(xmlSchematronValidCtxt *ctxt, int options);
XMLPUBFUN void xmlSchematronSetValidStructuredErrors( xmlSchematronValidCtxt *ctxt, xmlStructuredErrorFunc serror, void *ctx);
XMLPUBFUN int xmlSchematronValidCtxtGetOptions(xmlSchematronValidCtxt *ctxt);
XMLPUBFUN int xmlSchematronValidateOneElement (xmlSchematronValidCtxt *ctxt, xmlNode *elem);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_SCHEMATRON_H__ */
