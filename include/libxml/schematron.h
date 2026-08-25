/**
 * @file
 *
 * Schematron API for libxml-rs
 *
 * Native Rust implementation — drop-in replacement for libxml2's
 * schematron.h. API follows upstream libxml2 Schematron support.
 */

#ifndef __XML_SCHEMATRON_H__
#define __XML_SCHEMATRON_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * xmlSchematron:
 *
 * A Schematron schema (opaque type).
 */
typedef struct _xmlSchematron *xmlSchematronPtr;
typedef xmlSchematronPtr xmlSchematron;

/**
 * xmlSchematronParserCtxt:
 *
 * A Schematron parser context (opaque type).
 */
typedef struct _xmlSchematronParserCtxt *xmlSchematronParserCtxtPtr;
typedef xmlSchematronParserCtxtPtr xmlSchematronParserCtxt;

/**
 * xmlSchematronValidCtxt:
 *
 * A Schematron validation context (opaque type).
 */
typedef struct _xmlSchematronValidCtxt *xmlSchematronValidCtxtPtr;
typedef xmlSchematronValidCtxtPtr xmlSchematronValidCtxt;

/*
 * Parser functions
 */
xmlSchematronParserCtxtPtr xmlSchematronNewParserCtxt(const char *URL);
xmlSchematronParserCtxtPtr xmlSchematronNewMemParserCtxt(const char *buffer, int size);
xmlSchematronPtr           xmlSchematronParse(xmlSchematronParserCtxtPtr ctxt);
void                       xmlSchematronFreeParserCtxt(xmlSchematronParserCtxtPtr ctxt);
void                       xmlSchematronFree(xmlSchematronPtr schema);

/*
 * Validation functions
 */
xmlSchematronValidCtxtPtr  xmlSchematronNewValidCtxt(xmlSchematronPtr schema);
void                       xmlSchematronFreeValidCtxt(xmlSchematronValidCtxtPtr ctxt);
int                        xmlSchematronValidateDoc(xmlSchematronValidCtxtPtr ctxt,
                                                     xmlDocPtr doc);

#ifdef __cplusplus
}
#endif

#endif /* __XML_SCHEMATRON_H__ */
