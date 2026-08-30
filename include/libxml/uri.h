/**
 * @file
 *
 * URI handling API for libxml-rs
 *
 * # UPSTREAM-PARITY
 * `struct _xmlURI` layout matches upstream `uri.h` (libxml2 2.15.x).
 */

#ifndef __XML_URI_H__
#define __XML_URI_H__

#include <libxml/xmlversion.h>
#include <libxml/tree.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xmlURI xmlURI;
typedef xmlURI *xmlURIPtr;

struct _xmlURI {
    char *scheme;	/* the URI scheme */
    char *opaque;	/* opaque part */
    char *authority;	/* the authority part */
    char *server;	/* the server part */
    char *user;		/* the user part */
    int port;		/* the port number */
    char *path;		/* the path string */
    char *query;	/* the query string (deprecated - use with caution) */
    char *fragment;	/* the fragment identifier */
    int  cleanup;	/* parsing potentially unclean URI */
    char *query_raw;	/* the query string (as it appears in the URI) */
};

XMLPUBFUN xmlURIPtr xmlParseURI(const char *str);
XMLPUBFUN xmlURIPtr xmlParseURIRaw(const char *str, int raw);
XMLPUBFUN xmlURIPtr xmlParseURIReference(xmlURIPtr uri, const char *str);
XMLPUBFUN void xmlFreeURI(xmlURIPtr uri);
XMLPUBFUN xmlChar *xmlSaveUri(xmlURIPtr uri);
XMLPUBFUN int xmlURIUnescapeString(const char *str, int len, char *target);
XMLPUBFUN char *xmlURIEscapeStr(const char *str, const char *list);
XMLPUBFUN int xmlNormalizeURIPath(char *path);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN xmlChar * xmlBuildRelativeURI (const xmlChar *URI, const xmlChar *base);
XMLPUBFUN int xmlBuildRelativeURISafe (const xmlChar *URI, const xmlChar *base, xmlChar **out);
XMLPUBFUN xmlChar * xmlBuildURI (const xmlChar *URI, const xmlChar *base);
XMLPUBFUN int xmlBuildURISafe (const xmlChar *URI, const xmlChar *base, xmlChar **out);
XMLPUBFUN xmlChar* xmlCanonicPath (const xmlChar *path);
XMLPUBFUN xmlURI * xmlCreateURI (void);
XMLPUBFUN int xmlParseURISafe (const char *str, xmlURI **uri);
XMLPUBFUN xmlChar* xmlPathToURI (const xmlChar *path);
XMLPUBFUN void xmlPrintURI (FILE *stream, xmlURI *uri);
XMLPUBFUN xmlChar * xmlURIEscape (const xmlChar *str);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_URI_H__ */
