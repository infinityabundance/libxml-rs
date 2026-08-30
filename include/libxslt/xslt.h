/**
 * @file
 *
 * XSLT main header for libxml-rs
 *
 * # UPSTREAM-PARITY
 * Matches upstream `libxslt/xslt.h`: constants and version macros only.
 * Type declarations live in `xsltInternals.h` (upstream convention —
 * consumers include `<libxslt/xsltInternals.h>` for the engine types).
 */

#ifndef __XSLT_H__
#define __XSLT_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>
#include <libxslt/xsltexports.h>

#ifdef __cplusplus
extern "C" {
#endif

/* XSLT version */
#define LIBXSLT_DOTTED_VERSION "1.1.45"
#define LIBXSLT_VERSION 10145
#define LIBXSLT_VERSION_STRING "10145"
#define LIBXSLT_VERSION_EXTRA ""

/* XSLT 1.0 namespace (upstream xslt.h) */
#define XSLT_NAMESPACE ((const xmlChar *)"http://www.w3.org/1999/XSL/Transform")

/* Default parse options for loading XSLT documents (upstream xslt.h) */
#define XSLT_PARSE_OPTIONS \
 XML_PARSE_NOENT | XML_PARSE_DTDLOAD | XML_PARSE_DTDATTR | XML_PARSE_NOCDATA


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XSLTPUBVAR const char *xsltEngineVersion;
XSLTPUBVAR const int xsltLibxmlVersion;
XSLTPUBVAR const int xsltLibxsltVersion;
XSLTPUBVAR int xsltMaxDepth;
XSLTPUBVAR int xsltMaxVars;
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XSLT_H__ */
