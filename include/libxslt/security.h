/**
 * @file
 *
 * XSLT security API for libxml-rs.
 *
 * Upstream counterpart: libxslt/security.h (1.1.42 contract, R-000125).
 * The security model is callback-based: xsltSetSecurityPrefs registers an
 * xsltSecurityCheck function pointer per option.
 */

#ifndef __SECURITY_H__
#define __SECURITY_H__

#include <libxml/xmlversion.h>
#include <libxslt/xsltInternals.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct _xsltSecurityPrefs xsltSecurityPrefs;
typedef xsltSecurityPrefs *xsltSecurityPrefsPtr;

/**
 * xsltSecurityOption:
 *
 * The set of options that can be configured.
 */
typedef enum {
    XSLT_SECPREF_READ_FILE = 1,
    XSLT_SECPREF_WRITE_FILE,
    XSLT_SECPREF_CREATE_DIRECTORY,
    XSLT_SECPREF_READ_NETWORK,
    XSLT_SECPREF_WRITE_NETWORK
} xsltSecurityOption;

/**
 * xsltSecurityCheck:
 *
 * User-provided function to check the value of a string like a file
 * path or an URL. Returns non-zero to allow, 0 to deny.
 */
typedef int (*xsltSecurityCheck) (xsltSecurityPrefsPtr sec,
                                  xsltTransformContextPtr ctxt,
                                  const char *value);

/*
 * Module interfaces
 */
XSLTPUBFUN xsltSecurityPrefsPtr XSLTCALL
                xsltNewSecurityPrefs    (void);
XSLTPUBFUN void XSLTCALL
                xsltFreeSecurityPrefs   (xsltSecurityPrefsPtr sec);
XSLTPUBFUN int XSLTCALL
                xsltSetSecurityPrefs    (xsltSecurityPrefsPtr sec,
                                         xsltSecurityOption option,
                                         xsltSecurityCheck func);
XSLTPUBFUN xsltSecurityCheck XSLTCALL
                xsltGetSecurityPrefs    (xsltSecurityPrefsPtr sec,
                                         xsltSecurityOption option);

XSLTPUBFUN void XSLTCALL
                xsltSetDefaultSecurityPrefs (xsltSecurityPrefsPtr sec);
XSLTPUBFUN xsltSecurityPrefsPtr XSLTCALL
                xsltGetDefaultSecurityPrefs (void);

#ifdef __cplusplus
}
#endif

#endif /* __SECURITY_H__ */
