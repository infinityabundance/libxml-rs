/**
 * @file
 *
 * RELAX NG API for libxml-rs
 *
 * Native Rust implementation — drop-in replacement for libxml2's
 * relaxng.h. API follows upstream libxml2 RELAX NG support.
 */

#ifndef __XML_RELAXNG_H__
#define __XML_RELAXNG_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

/**
 * xmlRelaxNG:
 *
 * A RELAX NG schema (opaque type).
 */
typedef struct _xmlRelaxNG *xmlRelaxNGPtr;
typedef xmlRelaxNGPtr xmlRelaxNG;

/**
 * xmlRelaxNGParserCtxt:
 *
 * A RELAX NG parser context (opaque type).
 */
typedef struct _xmlRelaxNGParserCtxt *xmlRelaxNGParserCtxtPtr;
typedef xmlRelaxNGParserCtxtPtr xmlRelaxNGParserCtxt;

/**
 * xmlRelaxNGValidCtxt:
 *
 * A RELAX NG validation context (opaque type).
 */
typedef struct _xmlRelaxNGValidCtxt *xmlRelaxNGValidCtxtPtr;
typedef xmlRelaxNGValidCtxtPtr xmlRelaxNGValidCtxt;

/*
 * Parser functions
 */
xmlRelaxNGParserCtxtPtr xmlRelaxNGNewParserCtxt(const char *URL);
xmlRelaxNGParserCtxtPtr xmlRelaxNGNewMemParserCtxt(const char *buffer, int size);
xmlRelaxNGPtr           xmlRelaxNGParse(xmlRelaxNGParserCtxtPtr ctxt);
void                    xmlRelaxNGFreeParserCtxt(xmlRelaxNGParserCtxtPtr ctxt);
void                    xmlRelaxNGFree(xmlRelaxNGPtr schema);

/*
 * Validation functions
 */
xmlRelaxNGValidCtxtPtr  xmlRelaxNGNewValidCtxt(xmlRelaxNGPtr schema);
void                    xmlRelaxNGFreeValidCtxt(xmlRelaxNGValidCtxtPtr ctxt);
int                     xmlRelaxNGValidateDoc(xmlRelaxNGValidCtxtPtr ctxt, xmlDocPtr doc);
int                     xmlRelaxNGValidateFullElement(xmlRelaxNGValidCtxtPtr ctxt,
                                                      xmlDocPtr doc,
                                                      xmlNodePtr elem);

#ifdef __cplusplus
}
#endif

#endif /* __XML_RELAXNG_H__ */
