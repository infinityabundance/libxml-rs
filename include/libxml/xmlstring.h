/**
 * @file
 *
 * String utility functions for libxml-rs
 */

#ifndef __XML_STRING_H__
#define __XML_STRING_H__

#include <libxml/xmlversion.h>

/* va_list (xmlStrVPrintf, xmlVAsprintf) — upstream xmlstring.h relies on it */
#include <stdarg.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef unsigned char xmlChar;

#ifndef BAD_CAST
#define BAD_CAST (const xmlChar *)
#endif

XMLPUBFUN xmlChar *xmlStrdup(const xmlChar *cur);
XMLPUBFUN xmlChar *xmlStrndup(const xmlChar *cur, int len);
XMLPUBFUN int xmlStrlen(const xmlChar *str);
XMLPUBFUN int xmlStrcmp(const xmlChar *str1, const xmlChar *str2);
XMLPUBFUN int xmlStrncmp(const xmlChar *str1, const xmlChar *str2, int len);
XMLPUBFUN int xmlStrcasecmp(const xmlChar *str1, const xmlChar *str2);
XMLPUBFUN int xmlStrncasecmp(const xmlChar *str1, const xmlChar *str2, int len);
XMLPUBFUN int xmlStrEqual(const xmlChar *str1, const xmlChar *str2);
XMLPUBFUN int xmlStrQEqual(const xmlChar *pref, const xmlChar *name, const xmlChar *str);
XMLPUBFUN xmlChar *xmlStrcat(xmlChar *cur, const xmlChar *add);
XMLPUBFUN xmlChar *xmlStrncat(xmlChar *cur, const xmlChar *add, int len);
XMLPUBFUN xmlChar *xmlStrncatNew(const xmlChar *str1, const xmlChar *str2, int len);
XMLPUBFUN xmlChar *xmlStrcpy(xmlChar *dst, const xmlChar *src);
XMLPUBFUN xmlChar *xmlStrncpy(xmlChar *dst, const xmlChar *src, int len);
XMLPUBFUN xmlChar *xmlStrsub(const xmlChar *str, int start, int len);


/* [11.1-S] begin: oracle-extracted declarations
 * Extracted verbatim from the upstream headers (11.1-S header-surface
 * audit: every function the oracle headers declare must be declared by the
 * drop-in headers — the source-compatibility contract. Signatures are the upstream ABI contract.
 */
XMLPUBFUN xmlChar * xmlCharStrdup (const char *cur);
XMLPUBFUN xmlChar * xmlCharStrndup (const char *cur, int len);
XMLPUBFUN int xmlCheckUTF8 (const unsigned char *utf);
XMLPUBFUN int xmlGetUTF8Char (const unsigned char *utf, int *len);
XMLPUBFUN int xmlStrPrintf (xmlChar *buf, int len, const char *msg, ...) LIBXML_ATTR_FORMAT(3,4);
XMLPUBFUN int xmlStrVPrintf (xmlChar *buf, int len, const char *msg, va_list ap) LIBXML_ATTR_FORMAT(3,0);
XMLPUBFUN const xmlChar * xmlStrcasestr (const xmlChar *str, const xmlChar *val);
XMLPUBFUN const xmlChar * xmlStrchr (const xmlChar *str, xmlChar val);
XMLPUBFUN const xmlChar * xmlStrstr (const xmlChar *str, const xmlChar *val);
XMLPUBFUN int xmlUTF8Charcmp (const xmlChar *utf1, const xmlChar *utf2);
XMLPUBFUN int xmlUTF8Size (const xmlChar *utf);
XMLPUBFUN int xmlUTF8Strlen (const xmlChar *utf);
XMLPUBFUN int xmlUTF8Strloc (const xmlChar *utf, const xmlChar *utfchar);
XMLPUBFUN xmlChar * xmlUTF8Strndup (const xmlChar *utf, int len);
XMLPUBFUN const xmlChar * xmlUTF8Strpos (const xmlChar *utf, int pos);
XMLPUBFUN int xmlUTF8Strsize (const xmlChar *utf, int len);
XMLPUBFUN xmlChar * xmlUTF8Strsub (const xmlChar *utf, int start, int len);
/* [11.1-S] end: oracle-extracted declarations */

#ifdef __cplusplus
}
#endif

#endif /* __XML_STRING_H__ */
