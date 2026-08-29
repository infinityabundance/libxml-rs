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

#ifdef __cplusplus
}
#endif

#endif /* __XML_URI_H__ */
