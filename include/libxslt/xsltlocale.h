/**
 * @file
 *
 * XSLT locale API for libxml-rs
 *
 * Stub header — functions will be implemented in future phases.
 */

#ifndef __XSLTLOCALE_H__
#define __XSLTLOCALE_H__

#include <libxml/xmlversion.h>
#include <libxslt/xslt.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Functions will be declared here as they are implemented. */

XMLPUBFUN void *xsltNewLocale(const xmlChar *languageTag, int lowerFirst);
XMLPUBFUN void xsltFreeLocale(void *locale);
XMLPUBFUN void xsltFreeLocales(void);
XMLPUBFUN int xsltLocaleStrcmp(void *locale, const xmlChar *str1,
                               const xmlChar *str2);
XMLPUBFUN xmlChar *xsltStrxfrm(void *locale, const xmlChar *string);

#ifdef __cplusplus
}
#endif

#endif /* __XSLTLOCALE_H__ */
