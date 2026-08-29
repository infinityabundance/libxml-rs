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
#define LIBXSLT_DOTTED_VERSION "1.1.39"
#define LIBXSLT_VERSION 10139
#define LIBXSLT_VERSION_STRING "10139"
#define LIBXSLT_VERSION_EXTRA ""

/* XSLT 1.0 namespace (upstream xslt.h) */
#define XSLT_NAMESPACE ((const xmlChar *)"http://www.w3.org/1999/XSL/Transform")

/* Default parse options for loading XSLT documents (upstream xslt.h) */
#define XSLT_PARSE_OPTIONS \
 XML_PARSE_NOENT | XML_PARSE_DTDLOAD | XML_PARSE_DTDATTR | XML_PARSE_NOCDATA

#ifdef __cplusplus
}
#endif

#endif /* __XSLT_H__ */
