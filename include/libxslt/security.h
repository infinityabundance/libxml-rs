/**
 * @file
 *
 * XSLT security API for libxml-rs
 */

#ifndef __SECURITY_H__
#define __SECURITY_H__

#include <libxml/xmlversion.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void *xsltSecurityPrefsPtr;

XMLPUBFUN xsltSecurityPrefsPtr xsltNewSecurityPrefs(void);
XMLPUBFUN void xsltFreeSecurityPrefs(xsltSecurityPrefsPtr sec);
XMLPUBFUN int xsltSetSecurityPrefs(xsltSecurityPrefsPtr sec, int option, int value);
XMLPUBFUN int xsltGetSecurityPrefs(xsltSecurityPrefsPtr sec, int option);
XMLPUBFUN void xsltSetDefaultSecurityPrefs(xsltSecurityPrefsPtr sec);
XMLPUBFUN xsltSecurityPrefsPtr xsltGetDefaultSecurityPrefs(void);

#ifdef __cplusplus
}
#endif

#endif /* __SECURITY_H__ */
